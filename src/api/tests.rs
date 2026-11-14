use serde_json::json;

use super::*;

mod fixtures {
    use super::*;

    fn parse<T: serde::de::DeserializeOwned>(text: &str) -> T {
        serde_json::from_str(text).expect("fixture parses")
    }

    #[test]
    fn health() {
        let health: Health = parse(include_str!("testdata/health.json"));
        assert_eq!(health.msg, "OK");
        assert_eq!(health.version, MIN_SERVER_BUILD);
    }

    #[test]
    fn user() {
        let user: User = parse(include_str!("testdata/user.json"));
        assert_eq!(user.username, "tester");
    }

    #[test]
    fn households() {
        let households: Vec<Household> = parse(include_str!("testdata/households.json"));
        let household = &households[0];
        assert_eq!(household.name, "Testhaus");
        assert!(household.default_shopping_list.is_some());
        assert!(!household.member.is_empty());
    }

    #[test]
    fn shopping_lists() {
        let lists: Vec<ShoppingList> = parse(include_str!("testdata/shoppinglists.json"));
        let items: Vec<&ListItem> = lists.iter().flat_map(|l| &l.items).collect();
        assert!(!items.is_empty());
        assert!(items.iter().any(|i| i.description == "2 L"));
        assert!(items.iter().any(|i| i.item.category.is_some()));
        assert!(lists.iter().any(|l| !l.recent_items.is_empty()));
    }

    #[test]
    fn items_keep_icons() {
        let items: Vec<Item> = parse(include_str!("testdata/items.json"));
        assert!(
            items
                .iter()
                .any(|i| i.icon.as_deref() == Some("milk_carton"))
        );
    }

    #[test]
    fn categories() {
        let categories: Vec<Category> = parse(include_str!("testdata/categories.json"));
        assert!(categories.len() >= 2);
    }
}

mod addresses {
    use super::*;

    fn base(input: &str) -> String {
        Client::new(input).unwrap().base().to_string()
    }

    #[test]
    fn normalised_to_server_root() {
        assert_eq!(base("https://ko.example.net"), "https://ko.example.net/");
        assert_eq!(base("https://ko.example.net/"), "https://ko.example.net/");
        assert_eq!(
            base("https://ko.example.net/api"),
            "https://ko.example.net/"
        );
        assert_eq!(
            base("https://ko.example.net/api/"),
            "https://ko.example.net/"
        );
        assert_eq!(base("ko.example.net"), "https://ko.example.net/");
        assert_eq!(base("http://192.168.1.5:8080"), "http://192.168.1.5:8080/");
        assert_eq!(
            base("https://example.net/owl/api"),
            "https://example.net/owl/"
        );
    }

    #[test]
    fn rejects_non_web_addresses() {
        assert!(matches!(
            Client::new("ftp://example.net"),
            Err(Error::InvalidUrl(_))
        ));
        assert!(matches!(Client::new("https://"), Err(Error::InvalidUrl(_))));
    }
}

mod payloads {
    use super::*;

    #[test]
    fn item_update_sends_only_changed_fields() {
        let rename = ItemUpdate {
            name: Some("Oat milk".into()),
            ..Default::default()
        };
        assert_eq!(rename.payload(), json!({ "name": "Oat milk" }));

        let clear_category = ItemUpdate {
            category: Some(None),
            ..Default::default()
        };
        assert_eq!(clear_category.payload(), json!({ "category": null }));

        let set_category = ItemUpdate {
            category: Some(Some(4)),
            ..Default::default()
        };
        assert_eq!(set_category.payload(), json!({ "category": { "id": 4 } }));

        let clear_icon = ItemUpdate {
            icon: Some(None),
            ..Default::default()
        };
        assert_eq!(clear_icon.payload(), json!({ "icon": null }));
    }

    #[test]
    fn category_update_sends_only_changed_fields() {
        let reorder = CategoryUpdate {
            ordering: Some(3),
            ..Default::default()
        };
        assert_eq!(reorder.payload(), json!({ "ordering": 3 }));
    }
}

/// Runs against the server in `OWLET_TEST_SERVER`, signing in as `OWLET_TEST_USER` with
/// `OWLET_TEST_PASSWORD`. Skipped when unset. It writes; use a throwaway server.
mod socket {
    use super::*;

    fn frames() -> Vec<Frame> {
        let raw: Vec<String> =
            serde_json::from_str(include_str!("testdata/socket_frames.json")).unwrap();
        raw.iter().map(|f| parse_frame(f)).collect()
    }

    #[test]
    fn handshake() {
        let frames = frames();
        assert_eq!(
            frames[0],
            Frame::Open {
                ping_interval: 25000,
                ping_timeout: 20000
            }
        );
        assert_eq!(frames[1], Frame::Connected);
        assert_eq!(parse_frame("2"), Frame::Ping);
        assert_eq!(parse_frame("41"), Frame::Close);
        assert!(matches!(
            parse_frame("44{\"message\":\"x\"}"),
            Frame::ConnectError(_)
        ));
    }

    #[test]
    fn events() {
        let events: Vec<LiveEvent> = frames()
            .into_iter()
            .filter_map(|f| match f {
                Frame::Event(e) => Some(e),
                _ => None,
            })
            .collect();
        assert_eq!(events.len(), 5);
        let LiveEvent::ListAdded(list) = &events[0] else {
            panic!("expected a new list: {:?}", events[0]);
        };
        assert_eq!(list.name, "Socket Capture");
        let LiveEvent::ItemAdded { list: on, item } = &events[1] else {
            panic!("expected an added item: {:?}", events[1]);
        };
        assert_eq!(
            (on.id, item.item.name.as_str(), item.description.as_str()),
            (list.id, "Milk", "2 L")
        );
        assert!(
            matches!(&events[2], LiveEvent::ItemAdded { item, .. } if item.description == "1 L")
        );
        assert!(
            matches!(&events[3], LiveEvent::ItemRemoved { item_id, .. } if *item_id == item.item.id)
        );
        assert!(matches!(&events[4], LiveEvent::ListDeleted(l) if l.id == list.id));
        assert!(events.iter().all(|e| e.household_id() == list.household_id));
    }

    #[test]
    fn unknown_events_are_ignored() {
        assert_eq!(parse_frame("42[\"recipe:add\",{}]"), Frame::Other);
        assert_eq!(parse_frame("42not json"), Frame::Other);
        assert_eq!(parse_frame(""), Frame::Other);
    }
}

mod live {
    use super::*;

    struct Server {
        url: String,
        user: String,
        password: String,
    }

    fn server() -> Option<Server> {
        let var = |name| std::env::var(name).ok().filter(|v: &String| !v.is_empty());
        Some(Server {
            url: var("OWLET_TEST_SERVER")?,
            user: var("OWLET_TEST_USER")?,
            password: var("OWLET_TEST_PASSWORD")?,
        })
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        glib::MainContext::default().block_on(future)
    }

    /// One test, because each step depends on the server state the previous one left.
    #[test]
    fn full_round_trip() {
        let Some(server) = server() else {
            eprintln!("OWLET_TEST_SERVER not set; skipping the live API test");
            return;
        };
        block_on(round_trip(server));
    }

    async fn round_trip(server: Server) {
        let client = Client::new(&format!("{}/api", server.url)).unwrap();

        let health = client.check_server().await.unwrap();
        assert!(health.version >= MIN_SERVER_BUILD);

        let elsewhere = Client::new(&format!("{}/nothing-here", server.url)).unwrap();
        assert_eq!(elsewhere.health().await, Err(Error::NotKitchenOwl));

        let unreachable = Client::new("http://127.0.0.1:9").unwrap();
        assert!(unreachable.health().await.unwrap_err().is_network());

        assert_eq!(
            client
                .sign_in(&server.user, "wrong password", "Owlet test")
                .await,
            Err(Error::Unauthorized)
        );
        let signed_in = client
            .sign_in(&server.user, &server.password, "Owlet test")
            .await
            .unwrap();
        assert_eq!(signed_in.user.username, server.user);
        assert_eq!(client.user().await.unwrap().id, signed_in.user.id);

        let household = client.households().await.unwrap().remove(0);
        let default_list = household.default_shopping_list.clone().unwrap();

        lists(&client, household.id, default_list.id).await;
        catalog(&client, household.id, default_list.id).await;

        client.sign_out().await.unwrap();
        let revoked = client.clone();
        revoked.set_token(Some(signed_in.token));
        assert_eq!(revoked.user().await, Err(Error::Unauthorized));
    }

    async fn lists(client: &Client, household: i64, default_list: i64) {
        assert!(matches!(
            client.delete_list(default_list).await,
            Err(Error::Refused(_))
        ));

        let extra = client.add_list(household, "Costco").await.unwrap();
        let renamed = client.rename_list(extra.id, "Costco run").await.unwrap();
        assert_eq!(renamed.name, "Costco run");

        let milk = client
            .add_item_by_name(extra.id, "Milk", Some("2 L"))
            .await
            .unwrap();
        // Adding an item already on the list leaves its description alone.
        client
            .add_item_by_name(extra.id, "Milk", Some("1 L"))
            .await
            .unwrap();
        let on_list = list(client, household, extra.id).await;
        assert_eq!(on_list.items.len(), 1);
        assert_eq!(on_list.items[0].description, "2 L");

        client
            .set_description(extra.id, milk.id, "3 L")
            .await
            .unwrap();
        assert_eq!(
            list(client, household, extra.id).await.items[0].description,
            "3 L"
        );

        let checked_at = 1_790_000_000_000;
        client
            .remove_item(extra.id, milk.id, Some(checked_at))
            .await
            .unwrap();
        // Replaying a check-off already made elsewhere succeeds.
        client.remove_item(extra.id, milk.id, None).await.unwrap();
        let after = list(client, household, extra.id).await;
        assert!(after.items.is_empty());
        assert!(after.recent_items.iter().any(|i| i.item.id == milk.id));

        let bread = client
            .add_item_by_name(extra.id, "Bread", None)
            .await
            .unwrap();
        let eggs = client
            .add_item_by_name(extra.id, "Eggs", None)
            .await
            .unwrap();
        client
            .remove_items(extra.id, &[(bread.id, None), (eggs.id, Some(checked_at))])
            .await
            .unwrap();
        assert!(list(client, household, extra.id).await.items.is_empty());

        let by_order = client
            .shopping_lists(household, ListOrder::Algorithmic, 5)
            .await
            .unwrap();
        assert!(by_order.iter().any(|l| l.id == extra.id));

        client.delete_list(extra.id).await.unwrap();
        assert!(
            client
                .shopping_lists(household, ListOrder::Alphabetical, 0)
                .await
                .unwrap()
                .iter()
                .all(|l| l.id != extra.id)
        );
    }

    async fn catalog(client: &Client, household: i64, default_list: i64) {
        let dairy = client.add_category(household, "Dairy").await.unwrap();
        let bakery = client.add_category(household, "Bakery").await.unwrap();
        let bread_aisle = client.add_category(household, "Bread aisle").await.unwrap();

        let moved = client
            .update_category(
                bakery.id,
                &CategoryUpdate {
                    ordering: Some(0),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(moved.ordering, 0);
        let renamed = client
            .update_category(
                dairy.id,
                &CategoryUpdate {
                    name: Some("Dairy & eggs".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(renamed.name, "Dairy & eggs");

        let milk = client
            .create_item(household, "Whole milk", Some(dairy.id))
            .await
            .unwrap();
        assert_eq!(milk.category.as_ref().map(|c| c.id), Some(dairy.id));

        let with_icon = client
            .update_item(
                milk.id,
                &ItemUpdate {
                    icon: Some(Some("milk_carton".into())),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(with_icon.icon.as_deref(), Some("milk_carton"));
        let renamed = client
            .update_item(
                milk.id,
                &ItemUpdate {
                    name: Some("Milk, whole".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(renamed.icon.as_deref(), Some("milk_carton"));
        let uncategorised = client
            .update_item(
                milk.id,
                &ItemUpdate {
                    category: Some(None),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(uncategorised.category.is_none());
        assert_eq!(uncategorised.icon.as_deref(), Some("milk_carton"));
        let without_icon = client
            .update_item(
                milk.id,
                &ItemUpdate {
                    icon: Some(None),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(without_icon.icon.is_none());

        let duplicate = client
            .create_item(household, "Vollmilch", None)
            .await
            .unwrap();
        client
            .add_item_by_name(default_list, "Vollmilch", None)
            .await
            .unwrap();
        client
            .update_item(
                milk.id,
                &ItemUpdate {
                    merge_item_id: Some(duplicate.id),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let items = client.items(household).await.unwrap();
        assert!(items.iter().all(|i| i.id != duplicate.id));
        assert!(items.iter().any(|i| i.id == milk.id));

        let loaf = client
            .create_item(household, "Sourdough", Some(bread_aisle.id))
            .await
            .unwrap();
        client
            .update_category(
                bakery.id,
                &CategoryUpdate {
                    merge_category_id: Some(bread_aisle.id),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let categories = client.categories(household).await.unwrap();
        assert!(categories.iter().all(|c| c.id != bread_aisle.id));
        let loaf_now = item(client, household, loaf.id).await;
        assert_eq!(loaf_now.category.map(|c| c.id), Some(bakery.id));

        client.delete_category(bakery.id).await.unwrap();
        assert!(item(client, household, loaf.id).await.category.is_none());

        client.delete_item(loaf.id).await.unwrap();
        assert!(
            client
                .items(household)
                .await
                .unwrap()
                .iter()
                .all(|i| i.id != loaf.id)
        );

        // Leaving these behind makes the next run's create_item fail on the duplicate name.
        client.delete_item(milk.id).await.unwrap();
        client.delete_category(dairy.id).await.unwrap();
    }

    async fn list(client: &Client, household: i64, id: i64) -> ShoppingList {
        client
            .shopping_lists(household, ListOrder::Alphabetical, 10)
            .await
            .unwrap()
            .into_iter()
            .find(|l| l.id == id)
            .expect("list exists")
    }

    async fn item(client: &Client, household: i64, id: i64) -> Item {
        client
            .items(household)
            .await
            .unwrap()
            .into_iter()
            .find(|i| i.id == id)
            .expect("item exists")
    }
}
