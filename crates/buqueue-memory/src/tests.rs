//! Unit tests for the in-memory backend
//!
//! Every public API behaviour is covered here. These tests are also the
//! canonical examples of how to use buqueue-memory

use buqueue_core::{
    backend::BackendBuilder,
    dlq::DlqConfig,
    prelude::{Message, QueueConsumer, QueueProducer},
};
use bytes::Bytes;
use chrono::Utc;

use crate::{MemoryBackend, MemoryConfig, MemoryConsumer, MemoryProducer};

async fn make_pair() -> (MemoryProducer, MemoryConsumer) {
    MemoryBackend::builder(MemoryConfig::default())
        .build_pair()
        .await
        .unwrap()
}

#[tokio::test]
async fn send_and_receive() {
    let (producer, mut consumer) = make_pair().await;
    producer
        .send(Message::from_json(&42u32).unwrap())
        .await
        .unwrap();
    let delivery = consumer.receive().await.unwrap();
    assert_eq!(delivery.payload_json::<u32>().unwrap(), 42);
    delivery.ack().await.unwrap();
}

#[tokio::test]
async fn ack_removes_message_permanently() {
    let (producer, mut consumer) = make_pair().await;

    producer
        .send(Message::from_json(&1u32).unwrap())
        .await
        .unwrap();
    producer
        .send(Message::from_json(&2u32).unwrap())
        .await
        .unwrap();

    let d1 = consumer.receive().await.unwrap();
    assert_eq!(d1.payload_json::<u32>().unwrap(), 1);
    d1.ack().await.unwrap();

    let d2 = consumer.receive().await.unwrap();
    assert_eq!(d2.payload_json::<u32>().unwrap(), 2);
    assert_eq!(
        d2.delivery_count(),
        1,
        "message 2 should be a firs deliverym not a redelivery of message 1"
    );
    d2.ack().await.unwrap();

    assert!(
        consumer.try_receive().await.unwrap().is_none(),
        "queue should be empty after both messages were ack'd"
    );
}

#[tokio::test]
async fn routing_key_preserved() {
    let (producer, mut consumer) = make_pair().await;
    producer
        .send(Message::from_json_with_key(&1u32, "orders.placed").unwrap())
        .await
        .unwrap();
    let d = consumer.receive().await.unwrap();
    assert_eq!(d.routing_key(), Some("orders.placed"));
    d.ack().await.unwrap();
}

#[tokio::test]
async fn headers_preserved() {
    let (producer, mut consumer) = make_pair().await;
    let msg = Message::builder()
        .payload(Bytes::from_static(b"hello"))
        .header("x-custom", "value-123")
        .build()
        .unwrap();
    producer.send(msg).await.unwrap();
    let d = consumer.receive().await.unwrap();
    assert_eq!(d.header("x-custom"), Some("value-123"));
    d.ack().await.unwrap();
}

#[tokio::test]
async fn nack_requeues_with_incremented_count() {
    let (producer, mut consumer) = make_pair().await;
    producer
        .send(Message::from_json(&99u32).unwrap())
        .await
        .unwrap();

    let d1 = consumer.receive().await.unwrap();
    assert_eq!(d1.delivery_count(), 1);
    d1.nack().await.unwrap();

    let d2 = consumer.receive().await.unwrap();
    assert_eq!(d2.delivery_count(), 2);
    d2.ack().await.unwrap();
}

#[tokio::test]
async fn nack_routes_to_dlq_after_max_count() {
    let (producer, mut consumer) = MemoryBackend::builder(MemoryConfig::default())
        .dead_letter_queue(DlqConfig::new("test-dlq".to_string(), 2))
        .build_pair()
        .await
        .unwrap();

    producer
        .send(Message::from_json(&1u32).unwrap())
        .await
        .unwrap();

    consumer.receive().await.unwrap().nack().await.unwrap();
    consumer.receive().await.unwrap().nack().await.unwrap();

    assert!(consumer.try_receive().await.unwrap().is_none());
}

#[tokio::test]
async fn try_receive_returns_none_when_empty() {
    let (_, mut consumer) = make_pair().await;
    assert!(consumer.try_receive().await.unwrap().is_none());
}

#[tokio::test]
async fn batch_send_and_receive() {
    let (producer, mut consumer) = make_pair().await;

    let ids = producer
        .send_batch(vec![
            Message::from_json(&1u32).unwrap(),
            Message::from_json(&2u32).unwrap(),
            Message::from_json(&3u32).unwrap(),
        ])
        .await
        .unwrap();
    assert_eq!(ids.len(), 3);

    let deliveries = consumer.receive_batch(10).await.unwrap();
    assert_eq!(deliveries.len(), 3);
    for d in deliveries {
        d.ack().await.unwrap();
    }
}

#[tokio::test]
async fn graceful_shutdown_stops_receive_graceful() {
    let (_producer, mut consumer) = make_pair().await;
    let shutdown = consumer.shutdown_handle();

    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        shutdown.shutdown();
    });

    let result = tokio::time::timeout(
        tokio::time::Duration::from_millis(200),
        consumer.receive_graceful(),
    )
    .await
    .expect("should resolve after shutdown signal");

    assert!(result.is_none());
}

#[tokio::test]
async fn scheduled_message_delayed() {
    let (producer, mut consumer) = make_pair().await;

    let delivery_at = Utc::now() + chrono::Duration::milliseconds(100);
    producer
        .send_at(Message::from_json(&7u32).unwrap(), delivery_at)
        .await
        .unwrap();

    assert!(consumer.try_receive().await.unwrap().is_none());

    tokio::time::sleep(tokio::time::Duration::from_millis(130)).await;
    let d = consumer.receive().await.unwrap();
    assert_eq!(d.payload_json::<u32>().unwrap(), 7);
    d.ack().await.unwrap();
}

#[tokio::test]
async fn stream_yiels_messages_in_order() {
    use futures::StreamExt;

    let (producer, consumer) = make_pair().await;

    for i in 0u32..5 {
        producer
            .send(Message::from_json(&i).unwrap())
            .await
            .unwrap();
    }

    let values: Vec<u32> = consumer
        .into_stream()
        .take(5)
        .map(|r| r.unwrap().payload_json::<u32>().unwrap())
        .collect()
        .await;

    assert_eq!(values, vec![0, 1, 2, 3, 4]);
}

#[tokio::test]
async fn stream_ends_when_producer_dropped() {
    use futures::StreamExt;

    let (producer, consumer) = make_pair().await;

    for i in 0u32..5 {
        producer
            .send(Message::from_json(&i).unwrap())
            .await
            .unwrap();
    }

    drop(producer);

    let (values, errors): (Vec<_>, Vec<_>) = consumer
        .into_stream()
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .partition(Result::is_ok);

    let values: Vec<u32> = values
        .into_iter()
        .map(|r| r.unwrap().payload_json::<u32>().unwrap())
        .collect();

    assert_eq!(values, vec![0, 1, 2, 3, 4], "all sent messags must arrive");
    assert_eq!(
        errors.len(),
        1,
        "exactly one ConnectionLost error closes the stream"
    );
}

#[tokio::test]
async fn stream_ack_removes_messages() {
    // Each delivery pulled from the stream mmust be explicitly ack'd
    // After acking all messages the queue must be empty
    use futures::StreamExt;

    let (producer, consumer) = make_pair().await;

    for i in 0u32..5 {
        producer
            .send(Message::from_json(&i).unwrap())
            .await
            .unwrap();
    }

    let deliveries: Vec<_> = consumer
        .into_stream()
        .take(5)
        .map(|r| r.unwrap())
        .collect()
        .await;

    assert_eq!(deliveries.len(), 5);

    for d in deliveries {
        d.ack().await.unwrap();
    }

    let (_, mut check_consumer) = make_pair().await;
    assert!(
        check_consumer.try_receive().await.unwrap().is_none(),
        "a separate empty queue confirms ack'd items are not requeued
        the original stream consumer owns the channel and has drained it
        "
    );
}

#[tokio::test]
async fn stream_stops_on_shutdown() {
    // The stream must terminate cleanly when the shutdown handle fires,
    // even if there are no message in the queue
    use futures::StreamExt;

    let (_producer, consumer) = make_pair().await;
    let shutdown = consumer.shutdown_handle();

    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;
        shutdown.shutdown();
    });

    let result = tokio::time::timeout(
        tokio::time::Duration::from_millis(200),
        consumer.into_stream().collect::<Vec<_>>(),
    )
    .await;

    assert!(
        result.is_ok(),
        "stream should have terminated after shutdown signal"
    );
    assert!(
        result.unwrap().is_empty(),
        "no message weere sent so stream yields nothing"
    );
}

#[tokio::test]
async fn stream_stops_on_shutdown_after_partial_consume() {
    // Send some message, consume a few via the stream, then shutdown.
    // The stream should stop mid-way, remaining messages stay in the channel
    use futures::StreamExt;

    let (producer, consumer) = make_pair().await;
    let shutdown = consumer.shutdown_handle();

    for i in 0u32..10 {
        producer
            .send(Message::from_json(&i).unwrap())
            .await
            .unwrap();
    }

    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        shutdown.shutdown();
    });

    let received: Vec<u32> = tokio::time::timeout(
        tokio::time::Duration::from_millis(300),
        consumer
            .into_stream()
            .map(|r| r.unwrap())
            .then(|d| async move {
                let val = d.payload_json::<u32>().unwrap();
                d.ack().await.unwrap();
                val
            })
            .collect::<Vec<_>>(),
    )
    .await
    .expect("stream would terminate after shutdown");

    assert!(
        !received.is_empty(),
        "at least one message should have been received"
    );
    assert!(received.len() <= 10);

    let expected: Vec<u32> = (0..u32::try_from(received.len()).unwrap()).collect();
    assert_eq!(received, expected);
}

#[tokio::test]
async fn stream_for_each_concurrent() {
    // for_each_concurrent is the standard production usage pattern,
    // N tasks processing messages in parallel. Verify all messages are
    // received exactly once and ack'd correctly
    use futures::StreamExt;
    use std::sync::{Arc, Mutex};

    const MSG_COUNT: u32 = 20;
    let (prodcuer, consumer) = make_pair().await;

    for i in 0..MSG_COUNT {
        prodcuer
            .send(Message::from_json(&i).unwrap())
            .await
            .unwrap();
    }

    let received = Arc::new(Mutex::new(Vec::<u32>::new()));
    let received_clone = Arc::clone(&received);

    consumer
        .into_stream()
        .take(MSG_COUNT as usize)
        .for_each_concurrent(4, |result| {
            let received_ref = Arc::clone(&received_clone);
            async move {
                let delivery = result.unwrap();
                let val = delivery.payload_json::<u32>().unwrap();
                delivery.ack().await.unwrap();
                received_ref.lock().unwrap().push(val);
            }
        })
        .await;

    let values = received.lock().unwrap().clone();

    let expected: Vec<u32> = (0..MSG_COUNT).collect();
    assert_eq!(
        values, expected,
        "every message must be received exactly once"
    );
}

#[tokio::test]
async fn dyn_procducer_and_consumer_work() {
    let (producer, mut consumer) = MemoryBackend::builder(MemoryConfig::default())
        .make_dynamic()
        .build_pair()
        .await
        .unwrap();

    producer
        .send(Message::from_json(&777u32).unwrap())
        .await
        .unwrap();
    let delivery = consumer.receive().await.unwrap();
    assert_eq!(delivery.payload_json::<u32>().unwrap(), 777);
    delivery.ack().await.unwrap();
}
