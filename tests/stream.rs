#[cfg(feature = "async")]
use {
    flume::*,
    futures::{stream::FuturesUnordered, StreamExt, TryFutureExt},
    async_std::prelude::FutureExt,
    std::time::Duration,
};

#[cfg(feature = "async")]
#[test]
fn stream_recv() {
    let (tx, mut rx) = unbounded();

    let t = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(250));
        tx.send(42u32).unwrap();
        println!("sent");
    });

    async_std::task::block_on(async {
        println!("receiving...");
        let x = rx.stream().next().await;
        println!("received");
        assert_eq!(x, Some(42));
    });

    t.join().unwrap();
}

#[cfg(feature = "async")]
#[test]
fn stream_recv_disconnect() {
    let (tx, mut rx) = bounded::<i32>(0);

    let t = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(250));
        drop(tx)
    });

    async_std::task::block_on(async {
        assert_eq!(rx.stream().next().await, None);
    });

    t.join().unwrap();
}

#[cfg(feature = "async")]
#[test]
fn stream_recv_drop_recv() {
    let (tx, mut rx) = bounded::<i32>(10);

    let rx2 = rx.clone();
    let mut stream = rx.into_stream();

    async_std::task::block_on(async {
        let res = async_std::future::timeout(
            std::time::Duration::from_millis(500),
            stream.next()
        ).await;

        assert!(res.is_err());
    });

    let t = std::thread::spawn(move || {
        async_std::task::block_on(async {
            rx2.stream().next().await
        })
    });

    std::thread::sleep(std::time::Duration::from_millis(500));

    tx.send(42).unwrap();

    drop(stream);

    assert_eq!(t.join().unwrap(), Some(42))
}

#[cfg(feature = "async")]
#[async_std::test]
async fn stream_send_1_million_no_drop_or_reorder() {
    #[derive(Debug)]
    enum Message {
        Increment {
            old: u64,
        },
        ReturnCount,
    }

    let (tx, mut rx) = unbounded();

    let t = async_std::task::spawn(async move {
        let mut count = 0u64;
        let mut stream = rx.into_stream();

        while let Some(Message::Increment { old }) = stream.next().await {
            assert_eq!(old, count);
            count += 1;
        }

        count
    });

    for next in 0..1_000_000 {
        tx.send(Message::Increment { old: next }).unwrap();
    }

    tx.send(Message::ReturnCount).unwrap();

    let count = t.await;
    assert_eq!(count, 1_000_000)
}

#[cfg(feature = "async")]
#[async_std::test]
async fn parallel_streams_and_async_recv() {
    let (tx, rx) = flume::unbounded();
    let rx = &rx;
    let send_fut = async move {
        let n_sends: usize = 100000;
        for _ in 0..n_sends {
            tx.send_async(()).await.unwrap();
        }
    };

    async_std::task::spawn(
        send_fut
            .timeout(Duration::from_secs(5))
            .map_err(|_| panic!("Send timed out!"))
    );

    let mut futures_unordered = (0..250)
        .map(|n| async move {
            if n % 2 == 0 {
                let mut stream = rx.stream();
                while let Some(()) = stream.next().await {}
            } else {
                while let Ok(()) = rx.recv_async().await {}
            }

        })
        .collect::<FuturesUnordered<_>>();

    let recv_fut = async {
        while futures_unordered.next().await.is_some() {}
    };

    recv_fut
        .timeout(Duration::from_secs(5))
        .map_err(|_| panic!("Receive timed out!"))
        .await;
}

#[cfg(feature = "async")]
#[test]
fn stream_no_double_wake() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::pin::Pin;
    use std::task::Context;
    use futures::task::{waker, ArcWake};
    use futures::Stream;

    let mut count = Arc::new(AtomicUsize::new(0));

    // all this waker does is count how many times it is called
    struct CounterWaker {
        count: Arc<AtomicUsize>,
    }

    impl ArcWake for CounterWaker {
        fn wake_by_ref(arc_self: &Arc<Self>) {
            arc_self.count.fetch_add(1, Ordering::SeqCst);
        }
    }

    // create waker and context
    let w = CounterWaker {
        count: count.clone(),
    };
    let w = waker(Arc::new(w));
    let cx = &mut Context::from_waker(&w);

    // create unbounded channel
    let (tx, mut rx) = unbounded::<()>();
    let mut stream = rx.stream();

    // register waker with stream
    Pin::new(&mut stream).poll_next(cx);

    // send multiple items
    tx.send(()).unwrap();
    tx.send(()).unwrap();
    tx.send(()).unwrap();

    // verify that stream is only woken up once.
    assert_eq!(count.load(Ordering::SeqCst), 1);
}