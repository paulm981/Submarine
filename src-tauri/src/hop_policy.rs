//! The fork's rules for the hops of a jump route, kept out of lib.rs so the
//! hop code there stays the original project's (#169: `connect_jump_host`,
//! `config_jump_chain`, `reach_hop`, `login_hop`), with one-line calls into
//! this file.

#[cfg(test)]
mod tests {
    use crate::ssh_test_server::{client_over, connect, TestClient, TestServer};
    use russh::client;
    use russh::ChannelMsg;
    use std::time::Duration;

    /// Log in over `transport`, as the next hop of a route.
    async fn next_hop(transport: crate::tunnel::DrainedChannelStream) -> client::Handle<TestClient> {
        client_over(transport, client::Config::default(), TestClient::default()).await.expect("the next hop logs in")
    }

    /// Two hops, then a server behind them: a large download and upload at
    /// once go through, with no hop waiting on another for good.
    #[tokio::test]
    async fn a_two_hop_route_carries_traffic_both_ways_without_a_deadlock() {
        let hop1 = connect(TestServer::default(), |_| {}, client::Config::default()).await.unwrap();
        let hop2 = next_hop(crate::hop_channel(&hop1, "hop2", 22).await.unwrap()).await;
        let target = next_hop(crate::hop_channel(&hop2, "target", 22).await.unwrap()).await;

        const BYTES: usize = 6 * 1024 * 1024;
        let work = async {
            let mut flood = target.channel_open_session().await.unwrap();
            flood.exec(true, format!("flood {BYTES}")).await.unwrap();
            let mut echo = target.channel_open_session().await.unwrap();
            echo.exec(true, "echo").await.unwrap();
            let upload = vec![7u8; 2 * 1024 * 1024];
            let writer = echo.make_writer();
            let send = async move {
                let mut writer = writer;
                tokio::io::AsyncWriteExt::write_all(&mut writer, &upload).await.unwrap();
            };
            let read_flood = async {
                let mut got = 0;
                while let Some(msg) = flood.wait().await {
                    match msg {
                        ChannelMsg::Data { data } => got += data.len(),
                        ChannelMsg::Eof | ChannelMsg::Close => break,
                        _ => {}
                    }
                }
                got
            };
            let read_echo = async {
                let mut got = 0;
                while got < 2 * 1024 * 1024 {
                    match echo.wait().await {
                        Some(ChannelMsg::Data { data }) => got += data.len(),
                        Some(_) => {}
                        None => break,
                    }
                }
                got
            };
            let ((), flooded, echoed) = tokio::join!(send, read_flood, read_echo);
            (flooded, echoed)
        };
        let (flooded, echoed) = tokio::time::timeout(Duration::from_secs(60), work).await.expect("no deadlock across the hops");
        assert_eq!(flooded, BYTES);
        assert_eq!(echoed, 2 * 1024 * 1024);
    }

    /// A hop that never answers the channel open ends with an error in time,
    /// instead of holding the attempt.
    #[tokio::test]
    async fn a_hop_that_never_opens_the_channel_times_out() {
        let hop = connect(TestServer::stalling_direct_tcpip(), |_| {}, client::Config::default())
            .await
            .unwrap();
        let started = std::time::Instant::now();
        let err = match crate::hop_channel_within(&hop, "target", 22, Duration::from_millis(300)).await {
            Ok(_) => panic!("the channel can't open"),
            Err(e) => e,
        };
        assert!(err.contains("didn't open a channel to target:22"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
