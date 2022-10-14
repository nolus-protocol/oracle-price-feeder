#[cfg(test)]
mod tests {
    use std::fs;

    use wiremock::{matchers::path_regex, Mock, MockServer, ResponseTemplate};

    use crate::provider::{crypto::osmosis::OsmosisClient, Price, Provider};

    #[tokio::test]
    async fn get_all_pools_count() {
        // Arrange
        let mock_server = MockServer::start().await;
        let osmo_client = OsmosisClient::new(&mock_server.uri()).unwrap();

        let data = r#"{"numPools": "668"}"#;
        Mock::given(path_regex(r"^/num_pools$"))
            .respond_with(ResponseTemplate::new(200).set_body_string(data))
            .mount(&mock_server)
            .await;

        // Act
        let res = osmo_client.get_pools_count().await.unwrap();

        // Assert
        assert_eq!(res, 668);
    }

    #[tokio::test]
    async fn get_all_pools_fires_a_request_to_base_url() {
        // Arrange
        let mock_server = MockServer::start().await;
        let osmo_client = OsmosisClient::new(&mock_server.uri()).unwrap();

        let data = r#"{"numPools": "668"}"#;
        Mock::given(path_regex(r"^/num_pools$"))
            .respond_with(ResponseTemplate::new(200).set_body_string(data))
            .mount(&mock_server)
            .await;

        let data = fs::read_to_string("./tests/osmosis_pools_resp.json")
            .expect("Something went wrong reading the file");
        let template = ResponseTemplate::new(200).set_body_string(data);
        Mock::given(path_regex(r"^/pools$"))
            .respond_with(template)
            .expect(2)
            .mount(&mock_server)
            .await;

        // Act
        let res = osmo_client.get_pools(100).await.unwrap();

        // Assert
        assert_eq!(res.len(), 100);
        assert_eq!(
            res.get(0).unwrap().address,
            "osmo1mw0ac6rwlp5r8wapwk3zs6g29h8fcscxqakdzw9emkne6c8wjp9q0t3v8t"
        );

        let pool = res.get(15);
        assert_eq!(
            pool.unwrap().address,
            "osmo1hecg2sghe8y69el3r9s0ysvlgqrwhg626lwujq5wzh0hah8zsqgspe678n"
        );

        let pool = res.get(96);
        assert_eq!(pool.unwrap().id, "97");
        let res = osmo_client
            .get_spot_prices(&[vec![
                String::from(
                    "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2",
                ),
                String::from("uosmo"),
            ]])
            .await
            .unwrap();
        assert_eq!(
            vec![Price::new(
                "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2",
                5018906164562667503616000000,
                "uosmo",
                18050613591942994329600000000,
            )],
            res
        )
    }
}
