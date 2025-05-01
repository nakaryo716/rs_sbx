use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;
use tokio_tungstenite::accept_async;

#[tokio::main]
async fn main() {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:80").await.unwrap();

    // 複数の非同期タスク間でメッセージを共有するためのchannel
    let (chat_tx, _) = broadcast::channel(128);

    loop {
        let (stream, _) = listener.accept().await.unwrap();

        let chat_tx = chat_tx.clone();
        tokio::task::spawn(async move {
            let stream = accept_async(stream).await.unwrap();

            // ReaderとWriter用にWebSocketStreamをSplit
            let (mut tx, mut rx) = stream.split();

            let chat_tx_clone = chat_tx.clone();
            // Clientからのdataを受け取って、他の非同期タスクに伝えるタスク
            let mut recv_task = tokio::task::spawn(async move {
                while let Some(msg) = rx.next().await {
                    if let Ok(txt) = msg {
                        if let Err(e) = chat_tx_clone.send(txt) {
                            println!("recv-task:{:?}", e);
                            break;
                        }
                    }
                }
            });

            // 非同期間のメッセージをClientに送るタスク
            let mut send_task = tokio::task::spawn(async move {
                while let Ok(msg) = chat_tx.subscribe().recv().await {
                    if let Err(e) = tx.send(msg).await {
                        println!("send-task:{:?}", e);
                        break;
                    }
                }
            });

            // どちらかが終了したら、もう一方のタスクを中断させる
            tokio::select! {
                _ = &mut recv_task => {
                    send_task.abort();
                }
                _ = &mut send_task => {
                    recv_task.abort();
                }
            }
            println!("Connection closed");
        });
    }
}
