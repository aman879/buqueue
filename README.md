# buqueue

> **This project is under active development. The API may change before the first stable release.**

---

> **A production-grade, unified message queue abstraction for Rust.**
> One API. Kafka, NATS/JetStream, SQS, RabbitMQ, and Redis Streams —
> with scheduling, batching, DLQ, routing keys, and distributed tracing built in.

[![Crates.io](https://img.shields.io/crates/v/buqueue.svg)](https://crates.io/crates/buqueue)
[![docs.rs](https://img.shields.io/docsrs/buqueue)](https://docs.rs/buqueue)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/you/buqueue/actions/workflows/ci.yml/badge.svg)](https://github.com/you/buqueue/actions)

---

## Why buqueue?

The Rust ecosystem has excellent individual clients for Kafka, NATS, SQS, and RabbitMQ.
What it lacks is a **single abstraction** that lets you write your business logic once and
swap the broker underneath — without sacrificing the production features that real systems
need: scheduled delivery, batching, dead letter queues, routing, and distributed tracing.

buqueue fills that gap.

```
┌──────────────────────────────────────────────────────────┐
│                  Your application code                    │
│            (written against buqueue traits)               │
└──────────────┬───────────────────────┬────────────────────┘
               │    one unified API    │
┌──────────────▼───────────────────────▼────────────────────┐
│                       buqueue core                         │
│   QueueProducer · QueueConsumer · Message · DLQ · Routing │
└───┬──────────┬──────────┬───────────┬──────────┬──────────┘
    │          │          │           │          │
  Kafka   NATS/JS        SQS      RabbitMQ    Redis
                                              Streams
```

---

## A note on backend modes — why buqueue only wraps reliable transports

Some brokers offer two fundamentally different modes of operation. NATS has both Core
(fire-and-forget pub/sub, no persistence) and JetStream (persistent, acknowledged,
replayable). Redis has both Pub/Sub (fire-and-forget) and Streams (persistent, consumer
groups). buqueue **only wraps the persistent, acknowledgement-capable mode** of every
backend.

The reason is straightforward: buqueue's core contract promises at-least-once delivery —
`ack()` means the broker knows your message was processed, `nack()` means it gets
retried, and DLQ means nothing silently disappears. NATS Core and Redis Pub/Sub cannot
honour that contract because they have no acknowledgement and no persistence. Wrapping
them behind the same trait as Kafka would be lying to you.

| Backend  | Mode buqueue uses        | Mode buqueue excludes | Why excluded                   |
|----------|--------------------------|-----------------------|--------------------------------|
| Kafka    | Consumer Groups + Offsets | —                    | No split; Kafka is always persistent |
| NATS     | **JetStream only**       | NATS Core pub/sub     | No ack, no persistence         |
| SQS      | Standard + FIFO queues   | —                     | No split; SQS is always persistent |
| RabbitMQ | Classic + Quorum queues  | —                     | Both honour ack; choose via config |
| Redis    | **Streams only**         | Redis Pub/Sub         | No ack, no persistence         |

If you need fire-and-forget pub/sub, that is a legitimate need — it just belongs in a
different abstraction, not the same one as a reliable job queue.

---

## At a glance — what buqueue provides

| Feature                | Kafka | NATS/JS | SQS | RabbitMQ | Redis |
|------------------------|:-----:|:-------:|:---:|:--------:|:-----:|
| Send / Receive         |  ✅   |   ✅    | ✅  |    ✅    |  ✅   |
| Batch send/receive     |  ✅   |   ✅    | ✅  |    ✅    |  ✅   |
| Scheduled delivery     |  ✅   |   ✅    | ✅  |    ✅    |  ✅   |
| Dead Letter Queue      |  ✅   |   ✅    | ✅  |    ✅    |  ✅   |
| Message headers        |  ✅   |   ✅    | ✅  |    ✅    |  ✅   |
| Routing key / subject  |  ✅   |   ✅    | ✅  |    ✅    |  ✅   |
| Distributed tracing    |  ✅   |   ✅    | ✅  |    ✅    |  ✅   |
| Async Stream consumer  |  ✅   |   ✅    | ✅  |    ✅    |  ✅   |
| Graceful shutdown      |  ✅   |   ✅    | ✅  |    ✅    |  ✅   |

---

## Installation

Add buqueue to your `Cargo.toml`. Enable only the backends you need — you pay zero
compile cost for backends you don't use.

```toml
[dependencies]
buqueue = { version = "0.1", features = ["kafka", "tracing"] }

# All available feature flags:
#
#   backends : kafka | nats | sqs | rabbitmq | redis | memory
#   features : tracing | metrics
#
# Examples:
#   buqueue = { version = "0.1", features = ["sqs"] }
#   buqueue = { version = "0.1", features = ["kafka", "nats", "tracing", "metrics"] }
#   buqueue = { version = "0.1", features = ["rabbitmq", "tracing"] }
```

---

## Quickstart — Send and receive your first message

The simplest possible usage. Build a producer, send a message, receive it, acknowledge
it. Only the config struct changes between backends — everything else is identical.

```rust
use buqueue::prelude::*;
use buqueue::kafka::{KafkaBackend, KafkaConfig};

#[tokio::main]
async fn main() -> Result<(), BuqueueError> {
    // 1. Configure your backend. Every backend has its own strongly-typed config struct.
    //    There are no stringly-typed options maps — the compiler checks your config.
    let config = KafkaConfig {
        brokers: vec!["localhost:9092".to_string()],
        topic:   "my-topic".to_string(),
        group_id: "my-consumer-group".to_string(),
        ..Default::default()
    };

    // 2. Build a producer and consumer from the same config.
    //    build_pair() gives you both at once — the common case.
    let (producer, mut consumer) = KafkaBackend::builder(config)
        .build_pair()
        .await?;

    // 3. Build a message. The payload is raw bytes — you decide the encoding.
    //    Message::from_json() is the shortcut for serde-serialisable types.
    let msg = Message::from_json(&MyEvent { id: 1, name: "hello".into() })?;

    // 4. Send it.
    producer.send(msg).await?;

    // 5. Receive it. receive() blocks until a message is available.
    let delivery = consumer.receive().await?;

    // 6. Decode the payload back into your type.
    let event: MyEvent = delivery.payload_json()?;
    println!("Got: {:?}", event);

    // 7. Acknowledge — tells the broker the message was processed successfully.
    delivery.ack().await?;

    Ok(())
}
```

**The key insight:** swap `KafkaConfig` + `KafkaBackend` for `SqsConfig` + `SqsBackend`
and the rest of the code is completely unchanged. That is the entire value proposition.

---

## Message — the core type

A `Message` carries three things: a payload, headers, and metadata. Headers are the most
important addition over simpler libraries — they enable tracing propagation, routing,
idempotency checking, and schema versioning without touching the payload.

The `routing_key` field deserves special attention. Every backend has its own concept of
routing — Kafka has message keys (which determine partition), NATS has subjects, RabbitMQ
has routing keys and exchange bindings, SQS has message group IDs (for FIFO queues),
and Redis Streams stores it as a field. buqueue normalises all of these under one
`routing_key` field on `Message` so your code never needs to know which backend it is
talking to.

```rust
use buqueue::Message;
use bytes::Bytes;

// Build a message with full control over every field.
let msg = Message::builder()
    // Raw bytes payload — any encoding you like.
    .payload(Bytes::from_static(b"{\"order_id\": 99}"))
    // routing_key maps to: Kafka message key, NATS subject suffix,
    // RabbitMQ routing key, SQS MessageGroupId, Redis stream field.
    .routing_key("orders.placed.eu-west")
    // Arbitrary key-value headers — available on all five backends.
    .header("content-type", "application/json")
    .header("schema-version", "3")
    .header("source-service", "checkout")
    // Deduplication ID: SQS uses this for FIFO exactly-once dedup,
    // Kafka uses it as the idempotent producer key, others store it
    // as a header so your consumer can deduplicate manually.
    .deduplication_id("order-99281")
    .build();

// The JSON shortcut sets content-type automatically — the common case.
#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct OrderPlaced { order_id: u64, customer: String }

let msg = Message::from_json(&OrderPlaced {
    order_id: 99281,
    customer: "alice@example.com".into(),
})?;

// JSON with a routing key — the most common pattern in event-driven systems.
let msg = Message::from_json_with_key(
    &OrderPlaced { order_id: 99281, customer: "alice@example.com".into() },
    "orders.placed",
)?;
```

---

## Scheduled delivery — send a message for future processing

One of the most common production needs that most queue abstractions completely ignore.
`send_at()` takes a UTC timestamp and guarantees the message will not be visible to
consumers until that moment arrives. Each backend implements this differently under the
hood, and buqueue hides all of that complexity from you.

For reference: SQS uses its native delay seconds parameter (up to 15 minutes natively;
buqueue uses a holding queue for longer delays). RabbitMQ uses the
`rabbitmq-delayed-message-exchange` plugin. NATS JetStream uses its native publish-after
timestamp. Redis Streams uses a sorted set as a staging area with a background forwarder.
Kafka uses a delay topic with a buqueue-managed forwarder consumer. You never see any
of this — `send_at()` is the same on all five.

```rust
use buqueue::prelude::*;
use chrono::{Utc, Duration};

// Send a payment reminder 24 hours from now.
// Works identically on every backend — just change the config.
let deliver_at = Utc::now() + Duration::hours(24);

let msg = Message::from_json_with_key(
    &PaymentReminder { invoice_id: 42 },
    "reminders.payment",
)?;

producer.send_at(msg, deliver_at).await?;

// Your consumer code does not change at all. It just receives the message
// when it becomes visible, without knowing it was scheduled.
```

---

## Batch operations — high-throughput send and receive

Sending and receiving in batches dramatically reduces broker round-trips and costs. SQS
charges per API call — batching 10 messages into one call cuts your queue costs by 10x.
Kafka achieves its highest throughput through batching. For backends without native
batching (RabbitMQ), buqueue implements batching transparently so your code stays
consistent across all backends.

```rust
use buqueue::prelude::*;

// Batch send — buqueue uses each backend's native batch API where available.
// SQS: SendMessageBatch (up to 10). Kafka: producer batch. Others: pipelined.
let messages = vec![
    Message::from_json_with_key(&Event { id: 1 }, "events.created")?,
    Message::from_json_with_key(&Event { id: 2 }, "events.created")?,
    Message::from_json_with_key(&Event { id: 3 }, "events.updated")?,
];

let ids = producer.send_batch(messages).await?;
println!("Sent {} messages", ids.len());

// Batch receive — request up to N messages at once.
// Returns however many are currently available, from 1 up to the requested max.
let deliveries = consumer.receive_batch(10).await?;

for delivery in deliveries {
    let event: Event = delivery.payload_json()?;
    println!("Processing event {}", event.id);
    delivery.ack().await?;
}
```

---

## Dead Letter Queue — handle poison messages gracefully

A Dead Letter Queue receives messages that could not be processed after a configured
number of attempts. Without a DLQ, failed messages either block your queue or disappear
silently — both are unacceptable in production. buqueue makes DLQ configuration part of
the builder and gives you a dedicated consumer to inspect and redrive failed messages.

```rust
use buqueue::prelude::*;
use buqueue::sqs::{SqsBackend, SqsConfig};

let config = SqsConfig {
    queue_url: "https://sqs.us-east-1.amazonaws.com/123/orders".to_string(),
    region:    "us-east-1".to_string(),
    ..Default::default()
};

// DLQ is configured once at build time. After 3 failed nack()s the message
// is routed automatically — your consumer never handles this logic itself.
let (producer, mut consumer) = SqsBackend::builder(config)
    .dead_letter_queue(DlqConfig {
        destination:       "https://sqs.us-east-1.amazonaws.com/123/orders-dlq".to_string(),
        max_receive_count: 3,
    })
    .build_pair()
    .await?;

// Normal consumer code. nack() signals failure; buqueue tracks attempt counts.
let delivery = consumer.receive().await?;
if let Err(e) = process(&delivery).await {
    eprintln!("Processing failed: {e}");
    delivery.nack().await?;  // attempt 1 of 3 — visible again after visibility timeout
}

// ── Inspecting and redriving your DLQ ────────────────────────────────────────

let dlq_config = SqsConfig {
    queue_url: "https://sqs.us-east-1.amazonaws.com/123/orders-dlq".to_string(),
    region:    "us-east-1".to_string(),
    ..Default::default()
};

// A DLQ consumer is just a regular consumer pointed at the dead letter destination.
let mut dlq = SqsBackend::builder(dlq_config).build_consumer().await?;

let failed = dlq.receive().await?;

// delivery_count() tells you how many times this message was attempted.
println!("Failed after {} attempts", failed.delivery_count());
println!("First failed at: {:?}", failed.first_failed_at());

let original: OrderPlaced = failed.payload_json()?;
println!("Order that failed: {:?}", original);

// After investigating, redrive by sending back to the main queue.
producer.send(Message::from_json(&original)?).await?;
failed.ack().await?;  // Remove from DLQ.
```

---

## Async Stream consumer — compose with the full async ecosystem

For consumers that need backpressure, filtering, or composition with other async streams,
buqueue exposes every consumer as an async `Stream`. This lets you use the full power of
`StreamExt`, `tokio::select!`, and any async combinator in the ecosystem.

```rust
use buqueue::prelude::*;
use futures::StreamExt;

// into_stream() consumes the consumer and returns a Stream.
// Ownership transfer means you cannot accidentally double-receive.
let stream = consumer.into_stream();

// Combine with StreamExt to add filtering, limiting, concurrency, etc.
stream
    .take(100)                            // process exactly 100 messages then stop
    .filter_map(|result| async move {
        // Silently skip receive errors rather than crashing the whole loop.
        match result {
            Ok(d) => Some(d),
            Err(e) => { eprintln!("Receive error: {e}"); None }
        }
    })
    .for_each_concurrent(8, |delivery| async move {
        // Process up to 8 messages concurrently — each ack'd independently.
        let event: MyEvent = delivery.payload_json().unwrap();
        process(event).await;
        delivery.ack().await.unwrap();
    })
    .await;
```

---

## Distributed tracing — spans that cross queue boundaries

Without tracing support, a message queue is a black hole in your distributed traces —
the trace stops at the producer and your consumer looks like a cold start. buqueue
propagates the OpenTelemetry W3C trace context through message headers automatically,
so your traces flow through the queue as naturally as they flow through HTTP calls.

Enable the `tracing` feature flag — no other code changes are needed.

```toml
buqueue = { version = "0.1", features = ["kafka", "tracing"] }
```

```rust
// Producer side.
// buqueue reads the current span from the tracing context and injects it
// into the message headers as a W3C traceparent header. You write nothing.
let span = tracing::info_span!("handle_checkout", order_id = 99281);
let _guard = span.enter();

producer.send(msg).await?;
// ↑ traceparent header automatically written into message headers:
//   "traceparent: 00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"

// Consumer side.
// buqueue reads the traceparent header, reconstructs the remote span context,
// and starts a child span. Your consumer code runs inside that span.
// You write nothing on this side either.
let delivery = consumer.receive().await?;
// ↑ a child span is now active, parented to the producer's span.

// In your observability tool (Jaeger, Honeycomb, Datadog, etc.) you will see
// an unbroken trace chain:
//   HTTP handler → checkout service → [queue boundary] → order processor
// No gaps. No cold-start spans. Full end-to-end visibility.
```

---

## Graceful shutdown — drain in-flight messages before stopping

When your process receives SIGTERM during a deployment, you should finish processing
messages already in-flight before exiting. buqueue provides a `ShutdownHandle` for this.

```rust
use buqueue::prelude::*;
use tokio::signal;

let (producer, mut consumer) = KafkaBackend::builder(config)
    .build_pair()
    .await?;

// ShutdownHandle is lightweight and clone-able — send it wherever you need it.
let shutdown = consumer.shutdown_handle();

tokio::spawn(async move {
    signal::ctrl_c().await.unwrap();
    println!("SIGTERM received — draining in-flight messages...");
    // Signals the consumer to stop accepting new messages.
    // Waits until all in-flight deliveries have been ack'd or nack'd.
    shutdown.shutdown().await;
});

// receive_graceful() returns None when shutdown is complete — loop exits cleanly.
while let Some(delivery) = consumer.receive_graceful().await {
    let event: MyEvent = delivery.payload_json()?;
    process(event).await;
    delivery.ack().await?;
}

println!("Clean shutdown complete.");
```

---

## Backend configuration reference

This section documents every configuration option for all five backends, including
backend-specific modes, every field, and the reasoning behind each one. The goal is
that you should never need to read the upstream broker documentation just to configure
buqueue — everything you need to make an informed decision is here.

---

### Kafka

Kafka is always persistent and always uses consumer groups — there is no "lite mode"
to worry about. The main configuration decisions are which topic to use, which consumer
group to join, where to start reading if this is a brand-new consumer, and how to
handle compression. The `routing_key` on a `Message` maps to Kafka's **message key**,
which determines which partition the message lands in — messages with the same key
always go to the same partition, giving you ordering guarantees per key.

```rust
use buqueue::kafka::{KafkaBackend, KafkaConfig, OffsetReset, CompressionType};

let config = KafkaConfig {
    // One or more broker addresses. In production, list at least three for
    // redundancy. The Kafka client uses this list only for initial cluster
    // discovery — it learns the full topology from the brokers themselves.
    brokers: vec![
        "broker-1.internal:9092".to_string(),
        "broker-2.internal:9092".to_string(),
        "broker-3.internal:9092".to_string(),
    ],

    // The topic this producer/consumer operates on.
    topic: "order-events".to_string(),

    // Consumer group ID. All instances of your service share the same group_id —
    // Kafka distributes partitions across them automatically.
    // Different services reading the same topic use different group_ids so each
    // service gets its own independent copy of every message.
    group_id: "order-processor".to_string(),

    // What to do when a consumer joins a group for the first time with no
    // committed offset. Earliest = read from the beginning of the topic.
    // Latest = read only messages published from this point onward.
    // Default is Latest.
    offset_reset: OffsetReset::Latest,

    // Compress messages on the producer side. Snappy is the best default —
    // good ratio with low CPU overhead. Default is None (no compression).
    compression: CompressionType::Snappy,

    ..Default::default()
};

let (producer, mut consumer) = KafkaBackend::builder(config)
    .build_pair()
    .await?;
```

---

### NATS / JetStream

NATS has two modes: NATS Core (basic pub/sub, no persistence, fire-and-forget) and
JetStream (persistent, acknowledged, replayable). buqueue **exclusively uses JetStream**
because NATS Core cannot provide the at-least-once delivery contract that buqueue's
`ack()` and `nack()` API implies.

JetStream has two layers of configuration that you own: the **stream** (a persistent log
— which subjects it captures, how long it stores messages, how many replicas) and the
**consumer** (your service's view into that stream — where it starts reading, how long
it has to ack, how many messages it can hold in-flight). buqueue creates or verifies both
on startup, so you never need to pre-configure them in your NATS server manually.

The `routing_key` on a `Message` becomes the **NATS subject** it is published to. Your
stream's `subjects` list must cover all routing keys you plan to use — NATS uses
wildcard matching (`>` matches anything, `*` matches one token).

```rust
use buqueue::nats::{NatsBackend, NatsConfig, JetStreamConfig, StorageType,
                    RetentionPolicy, AckPolicy, DeliverPolicy};
use std::time::Duration;

let config = NatsConfig {
    // NATS server URL. For a cluster, list multiple servers comma-separated
    // or point to your cluster's load balancer.
    url: "nats://localhost:4222".to_string(),

    // Path to a NATS credentials file for authenticated servers.
    // Leave as None for unauthenticated local development.
    credentials_file: None, // Some("/etc/nats/service.creds".to_string())

    // ── JetStream stream configuration ───────────────────────────────────────
    // Defines the persistent log that buqueue publishes to and consumes from.
    // buqueue creates this stream if it does not exist, or validates it matches
    // this config if it already does.
    jetstream: JetStreamConfig {
        // Stream name — must be unique within a NATS account.
        // Convention: SCREAMING_SNAKE_CASE, named after the data it contains.
        stream_name: "ORDER_EVENTS".to_string(),

        // Which subjects this stream captures. routing_key on Message becomes
        // the subject the message is published to — so these patterns must
        // cover all routing keys you intend to use.
        // "orders.>" captures orders.placed, orders.updated, orders.cancelled, etc.
        subjects: vec!["orders.>".to_string()],

        // Where JetStream physically stores messages.
        // File: survives server restarts. Use this in production.
        // Memory: faster, but lost on server restart. Use this in tests only.
        storage: StorageType::File,

        // How long to retain messages from the time they were published.
        // After this window elapses, messages are deleted regardless of whether
        // they were consumed. Think of it as a TTL on the stream.
        max_age: Duration::from_secs(60 * 60 * 24 * 7), // 7 days

        // Retention policy — what triggers message deletion.
        //
        // Limits: messages are deleted when the stream hits size/age/count limits.
        //   Use this when you want a replay log — multiple consumer groups can
        //   each read the full history independently (like Kafka consumer groups).
        //
        // WorkQueue: messages are deleted as soon as they are acknowledged.
        //   Use this for job queues where each message should be processed by
        //   exactly one consumer and then discarded. More storage-efficient.
        //
        // Interest: messages are deleted when all registered consumers have acked.
        //   Use this when you need fan-out AND you want the stream to clean up
        //   automatically once every interested party has consumed a message.
        retention: RetentionPolicy::WorkQueue,

        // Maximum messages to keep in the stream. -1 means no limit.
        max_messages: -1,

        // Maximum total bytes across all stored messages. -1 means no limit.
        max_bytes: -1,

        // Maximum size of any single message. -1 means server default (usually 1MB).
        // Set this to match your NATS server's max_payload configuration.
        max_message_size: -1,

        // Number of stream replicas across a NATS cluster.
        // Must be 1 for single-node. Use 3 for a production cluster.
        // Ignored gracefully on single-node setups.
        num_replicas: 1,

        ..Default::default()
    },

    // ── JetStream consumer (durable) configuration ───────────────────────────
    // Defines how YOUR service reads from the stream above.

    // Durable consumer name. "Durable" means NATS remembers your read position
    // if you disconnect and reconnect. All instances of your service share the
    // same durable_consumer name — NATS load-balances messages across them.
    // Different services use different names to get independent message copies.
    durable_consumer: "order-processor".to_string(),

    // Where to start reading when this consumer is first created.
    // New: only messages published after this consumer was registered.
    // All: replay from the very beginning of the stream.
    // Default is New.
    deliver_policy: DeliverPolicy::New,

    // Must be Explicit in buqueue. Each message must be individually ack'd.
    // The other option (None — no ack required) defeats the purpose of
    // using JetStream and is not supported.
    ack_policy: AckPolicy::Explicit,

    // How long the server waits for an ack before redelivering the message
    // to another consumer. Set this generously above your expected processing
    // time — if you are unsure, start with 30 seconds and tune from there.
    ack_wait: Duration::from_secs(30),

    // Maximum unacknowledged messages this consumer can have in-flight at once.
    // This provides backpressure — the server stops sending new messages once
    // this limit is reached until some are ack'd. -1 means unlimited.
    max_ack_pending: 1000,
};

let (producer, mut consumer) = NatsBackend::builder(config)
    .build_pair()
    .await?;
```

---

### AWS SQS

SQS comes in two queue types: **Standard** (best-effort ordering, at-least-once delivery,
very high throughput — the right default for most workloads) and **FIFO** (strict
ordering per message group, exactly-once deduplication within a 5-minute window, lower
throughput). You choose the type when you create the queue in AWS — buqueue detects
which mode you are in from whether your `queue_url` ends in `.fifo`.

The most important SQS-specific concept is the **visibility timeout**: when a consumer
receives a message, SQS hides it from other consumers for a configurable window. If your
consumer crashes before calling `ack()`, SQS makes the message visible again after the
window expires. Set it well above your expected processing time. For long-running jobs,
call `delivery.extend_visibility(duration).await?` mid-processing to avoid accidental
redelivery.

In FIFO mode, `routing_key` on `Message` maps to SQS's **MessageGroupId** — all messages
with the same group ID are delivered strictly in the order they were sent.

```rust
use buqueue::sqs::{SqsBackend, SqsConfig, VisibilityTimeout};

// ── Standard Queue ───────────────────────────────────────────────────────────

let config = SqsConfig {
    // The full SQS queue URL from your AWS console or Terraform output.
    queue_url: "https://sqs.us-east-1.amazonaws.com/123456789012/order-events".to_string(),

    // AWS region the queue lives in.
    region: "us-east-1".to_string(),

    // How long a received message is hidden from other consumers while you
    // process it. If you crash before ack()ing, SQS redelivers after this window.
    // Set it well above your expected processing time.
    // You can extend it mid-processing: delivery.extend_visibility(Duration::from_secs(60)).await?
    visibility_timeout: VisibilityTimeout::Seconds(30),

    // Override the SQS endpoint — essential for local testing with LocalStack.
    // Leave as None in production.
    endpoint_url: None, // Some("http://localhost:4566".to_string()) for LocalStack

    // AWS credentials are resolved automatically from the environment:
    // AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY, AWS_PROFILE, IAM instance role,
    // or ECS task role — whichever is present. Never put credentials in config.

    ..Default::default()
};

let (producer, mut consumer) = SqsBackend::builder(config).build_pair().await?;

// ── FIFO Queue ───────────────────────────────────────────────────────────────
// FIFO queues use the same SqsConfig struct — the .fifo suffix on the URL
// signals FIFO mode to both SQS and buqueue.

let fifo_config = SqsConfig {
    queue_url: "https://sqs.us-east-1.amazonaws.com/123456789012/order-events.fifo"
        .to_string(),
    region: "us-east-1".to_string(),
    visibility_timeout: VisibilityTimeout::Seconds(30),
    ..Default::default()
};

// In FIFO mode, routing_key = MessageGroupId.
// All messages for the same customer are processed in the order they were sent.
// deduplication_id = MessageDeduplicationId.
// SQS will not deliver a duplicate within a 5-minute window.
let msg = Message::from_json_with_key(
    &OrderPlaced { order_id: 99281 },
    "customer-7821",  // all orders for this customer arrive in order
)?
.with_deduplication_id("order-99281");

let (producer, mut consumer) = SqsBackend::builder(fifo_config).build_pair().await?;
```

---

### RabbitMQ

RabbitMQ has two queue types that both honour the acknowledgement contract and work
with buqueue: **Classic queues** (the original RabbitMQ queue, fully mature, good for
most workloads) and **Quorum queues** (Raft-based consensus, survives node failures,
the recommended choice for new production deployments on a cluster).

The other key RabbitMQ concept is the **exchange**. In RabbitMQ, producers never publish
directly to a queue — they publish to an exchange with a routing key, and the exchange
routes to one or more queues based on binding rules. buqueue exposes the full exchange
model via `ExchangeConfig`. The `routing_key` on a `Message` becomes the AMQP routing
key sent to the exchange.

If you don't configure an exchange, buqueue publishes to the default exchange (which
routes directly to the queue by name) — this is the simplest setup and perfectly fine
for most applications.

```rust
use buqueue::rabbitmq::{RabbitMqBackend, RabbitMqConfig, QueueType,
                        ExchangeConfig, ExchangeKind};
use std::time::Duration;

// ── Direct to queue — no exchange (the simplest setup) ───────────────────────

let simple_config = RabbitMqConfig {
    // Full AMQP URI. %2F is the URL-encoded default virtual host '/'.
    url: "amqp://guest:guest@localhost:5672/%2F".to_string(),

    // Queue to publish to and consume from.
    // buqueue declares this queue on startup (idempotent — safe if it exists).
    queue: "order-events".to_string(),

    // Classic: original queue type, stable, broadly compatible.
    // Quorum: Raft-based, recommended for production clusters of 3+ nodes.
    //   Quorum queues require RabbitMQ 3.8+ and provide stronger durability
    //   guarantees — they survive the loss of a minority of cluster nodes.
    //   On a single-node setup, both types behave identically.
    queue_type: QueueType::Quorum,

    // Whether the queue survives a broker restart. Always true in production.
    // Quorum queues are always durable — this flag is ignored for them.
    durable: true,

    // No exchange — messages route directly to the named queue.
    exchange: None,

    // How long to wait for the initial connection before giving up.
    connection_timeout: Duration::from_secs(10),

    // Maximum unacknowledged messages this consumer holds at once (prefetch_count).
    // Set this to match your concurrency level — if you process 8 messages
    // concurrently, set this to 8. Lower = more fair distribution across
    // consumer instances. Higher = more throughput per instance.
    prefetch_count: 10,

    ..Default::default()
};

// ── Topic exchange — route by subject pattern (the production standard) ──────
// routing_key on Message becomes the AMQP routing key sent to the exchange.
// The exchange matches it against binding keys to decide which queue(s) to route to.
// Topic exchange binding keys use dot-separated words with wildcards:
//   '*' matches exactly one word.    "orders.*.placed" matches "orders.eu.placed"
//   '#' matches zero or more words.  "orders.#"        matches "orders.eu.placed.v2"

let exchange_config = RabbitMqConfig {
    url: "amqp://guest:guest@localhost:5672/%2F".to_string(),
    queue: "order-events".to_string(),
    queue_type: QueueType::Quorum,
    durable: true,
    connection_timeout: Duration::from_secs(10),
    prefetch_count: 10,

    exchange: Some(ExchangeConfig {
        // Exchange name — shared across all services that publish or consume from it.
        name: "domain-events".to_string(),

        // Topic: dot-separated wildcard routing — the most flexible and most
        // common choice for event-driven systems.
        // Direct: exact routing key match only — simpler but less flexible.
        // Fanout: send to ALL bound queues regardless of routing key.
        //   Use Fanout for broadcasting events to multiple independent services.
        // Headers: route based on header values instead of routing key — rarely needed.
        kind: ExchangeKind::Topic,

        // Binding key — the pattern that tells the exchange to route to THIS queue.
        // "orders.#" matches any routing key that starts with "orders." so this
        // queue receives orders.placed, orders.updated, orders.cancelled, etc.
        binding_key: "orders.#".to_string(),

        // Exchange also survives a broker restart.
        durable: true,
    }),

    ..Default::default()
};

let (producer, mut consumer) = RabbitMqBackend::builder(exchange_config)
    .build_pair()
    .await?;

// This message will be routed by the exchange based on its routing key.
// "orders.placed" matches the binding "orders.#" and lands in our queue.
let msg = Message::from_json_with_key(
    &OrderPlaced { order_id: 99281 },
    "orders.placed",
)?;
producer.send(msg).await?;
```

---

### Redis Streams

Redis Streams is Redis's persistent, acknowledged message log — it is completely
different from Redis Pub/Sub (which is fire-and-forget with no ack). buqueue **only uses
Redis Streams**, not Pub/Sub, because Streams are the only Redis messaging primitive
that can provide the at-least-once delivery contract.

Redis Streams uses a consumer group model very similar to Kafka: all instances of your
service share a group name, and Redis distributes messages across them. Each message must
be explicitly acknowledged with `XACK`. If a consumer crashes before acking, the message
stays in a "pending" state and can be claimed by another consumer after a configurable
timeout — buqueue handles this reclaim logic automatically.

```rust
use buqueue::redis::{RedisBackend, RedisConfig};
use std::time::Duration;

let config = RedisConfig {
    // Redis connection URL.
    // Standalone:  redis://[:password@]host[:port][/db]
    // TLS:         rediss://host:port
    // Sentinel:    redis+sentinel://host:port/service-name
    url: "redis://localhost:6379".to_string(),

    // The Redis stream key — a regular Redis key that buqueue will use as the
    // stream. Namespacing with colons is a Redis convention.
    // buqueue creates this stream on first use with XADD if it does not exist.
    stream: "buqueue:order-events".to_string(),

    // Consumer group name. All instances of your service share this name —
    // Redis routes each message to exactly one instance in the group.
    // Different services that want to read the same stream use different group
    // names so each service gets its own independent copy of every message.
    // buqueue creates the group with XGROUP CREATE on startup if needed.
    group: "order-processor".to_string(),

    // Consumer name — must be unique per running instance within the group.
    // Redis tracks the pending message list per consumer name, so it can
    // redeliver messages if this instance crashes. Use your pod name,
    // hostname, or a UUID generated at startup.
    consumer: std::env::var("HOSTNAME").unwrap_or_else(|_| "instance-1".to_string()),

    // Maximum messages to fetch per XREADGROUP call.
    // This is the batch size used by receive_batch(). receive() always returns one.
    read_count: 100,

    // How long (milliseconds) to block on XREADGROUP waiting for new messages.
    // 0 = block indefinitely until a message arrives.
    // > 0 = return after this many ms even if no messages, allowing your loop
    //   to check other conditions (shutdown signal, health checks, etc.).
    block_ms: 2000,

    // How long a message can be pending (delivered but not ack'd) before buqueue
    // considers it stale and uses XAUTOCLAIM to reassign it to another consumer.
    // This is your crash-recovery window — set it well above your expected
    // processing time. If processing takes up to 10 seconds, use 60 seconds.
    pending_timeout: Duration::from_secs(60),
};

let (producer, mut consumer) = RedisBackend::builder(config)
    .build_pair()
    .await?;
```

---

## Dynamic dispatch — choose your backend at runtime

For applications that select a backend from an environment variable or config file,
buqueue supports returning trait objects with one extra builder call.

```rust
// Static dispatch — the compiler knows the exact backend type at compile time.
// Zero overhead. Use this when your backend is fixed at build time.
let (producer, consumer) = KafkaBackend::builder(cfg).build_pair().await?;

// Dynamic dispatch — backend selected at runtime.
// Costs one pointer indirection. Enables storing producers in structs without
// generic parameters and passing to non-generic functions.
let (producer, consumer) = KafkaBackend::builder(cfg)
    .make_dynamic()   // returns (Box<dyn QueueProducer>, Box<dyn QueueConsumer>)
    .build_pair()
    .await?;

// Example: select backend from an environment variable at startup.
let (producer, consumer): (Box<dyn QueueProducer>, Box<dyn QueueConsumer>) =
    match std::env::var("QUEUE_BACKEND").as_deref() {
        Ok("kafka")    => KafkaBackend::builder(kafka_cfg).make_dynamic().build_pair().await?,
        Ok("sqs")      => SqsBackend::builder(sqs_cfg).make_dynamic().build_pair().await?,
        Ok("nats")     => NatsBackend::builder(nats_cfg).make_dynamic().build_pair().await?,
        Ok("rabbitmq") => RabbitMqBackend::builder(rmq_cfg).make_dynamic().build_pair().await?,
        _              => MemoryBackend::builder(Default::default()).make_dynamic().build_pair().await?,
    };
```

---

## Error handling

buqueue uses a single `BuqueueError` type with a structured `ErrorKind` that lets you
distinguish transient failures (safe to retry) from permanent ones (worth alerting on).

```rust
use buqueue::{BuqueueError, ErrorKind};

match producer.send(msg).await {
    Ok(id) => {
        println!("Sent: {id}");
    }

    Err(BuqueueError { kind: ErrorKind::ConnectionLost, .. }) => {
        // Transient. buqueue reconnects automatically in the background.
        // You may want to retry the send after a short backoff.
        eprintln!("Broker connection lost — retrying...");
    }

    Err(BuqueueError { kind: ErrorKind::PayloadTooLarge, .. }) => {
        // Permanent. This message will never succeed regardless of retries.
        // Log it, send to an error channel, or alert.
        eprintln!("Payload too large for broker — dropping message");
    }

    Err(BuqueueError { kind: ErrorKind::AuthenticationFailed, .. }) => {
        // Permanent and urgent — credentials are wrong or expired.
        panic!("Queue authentication failed — check credentials");
    }

    Err(BuqueueError { kind: ErrorKind::BackendSpecific { code, message }, .. }) => {
        // An error from the backend that buqueue doesn't classify specifically.
        // code is the raw backend error code; message is human-readable.
        eprintln!("Backend error {code}: {message}");
    }

    Err(e) => return Err(e.into()),
}
```

---

## Testing with the in-memory backend

Every component that takes a `QueueProducer` or `QueueConsumer` can be tested without
any broker running. The in-memory backend is backed by a `tokio::mpsc` channel and
supports the full buqueue API — scheduling, batching, DLQ, routing keys, and graceful
shutdown all work exactly as they do in production.

```rust
#[cfg(test)]
mod tests {
    use buqueue::memory::{MemoryBackend, MemoryConfig};
    use buqueue::prelude::*;

    #[tokio::test]
    async fn test_order_processor() {
        // No Docker. No environment variables. No network. No cleanup needed.
        let (producer, mut consumer) = MemoryBackend::builder(MemoryConfig::default())
            .build_pair()
            .await
            .unwrap();

        producer.send(
            Message::from_json_with_key(&OrderPlaced { order_id: 1 }, "orders.placed").unwrap()
        ).await.unwrap();

        let delivery = consumer.receive().await.unwrap();
        let order: OrderPlaced = delivery.payload_json().unwrap();

        assert_eq!(order.order_id, 1);
        assert_eq!(delivery.routing_key(), Some("orders.placed"));
        delivery.ack().await.unwrap();
    }

    #[tokio::test]
    async fn test_dlq_after_max_retries() {
        // The in-memory backend honours DLQ config just like a real broker.
        let (producer, mut consumer) = MemoryBackend::builder(MemoryConfig::default())
            .dead_letter_queue(DlqConfig {
                destination:       "test-dlq".to_string(),
                max_receive_count: 2,
            })
            .build_pair()
            .await
            .unwrap();

        producer.send(Message::from_json(&BadMessage {}).unwrap()).await.unwrap();

        // First attempt — nack.
        let d1 = consumer.receive().await.unwrap();
        d1.nack().await.unwrap();

        // Second attempt — nack again. Exhausts max_receive_count.
        let d2 = consumer.receive().await.unwrap();
        assert_eq!(d2.delivery_count(), 2);
        d2.nack().await.unwrap();

        // Main queue is now empty — message has moved to the DLQ.
        assert!(consumer.try_receive().await.unwrap().is_none());
    }
}
```

---

## Migration from omniqueue

The concepts map directly. buqueue adds headers, routing keys, scheduling, batch
operations, DLQ configuration, and tracing — none of which require you to change your
core send/receive logic.

```rust
// omniqueue (before)
use omniqueue::backends::{SqsBackend, SqsConfig};
let cfg = SqsConfig { queue_dsn: "...".to_string(), override_endpoint: false };
let (producer, mut consumer) = SqsBackend::builder(cfg).build_pair().await?;
producer.send_serde_json(&my_event).await?;
let delivery = consumer.receive().await?;
let event = delivery.payload_serde_json::<MyEvent>().await?;
delivery.ack().await?;

// buqueue (after) — reads nearly identically, but with full production features available
use buqueue::sqs::{SqsBackend, SqsConfig};
let cfg = SqsConfig { queue_url: "...".to_string(), region: "us-east-1".to_string(), ..Default::default() };
let (producer, mut consumer) = SqsBackend::builder(cfg).build_pair().await?;
producer.send(Message::from_json(&my_event)?).await?;
let delivery = consumer.receive().await?;
let event: MyEvent = delivery.payload_json()?;
delivery.ack().await?;
```

---

## Workspace structure (for contributors)

buqueue is a Cargo workspace. Each backend is its own crate — you compile only what you
enable via feature flags.

```
buqueue/
├── buqueue-core/        ← QueueProducer, QueueConsumer, Message, Delivery,
│                           BuqueueError, DlqConfig — zero backend dependencies.
│                           Depend on this crate directly if you are writing
│                           a third-party backend for buqueue.
├── buqueue-kafka/       ← Kafka backend        (depends on: rdkafka)
├── buqueue-nats/        ← NATS/JetStream        (depends on: async-nats)
├── buqueue-sqs/         ← AWS SQS              (depends on: aws-sdk-sqs)
├── buqueue-rabbitmq/    ← RabbitMQ             (depends on: lapin)
├── buqueue-redis/       ← Redis Streams        (depends on: redis)
├── buqueue-memory/      ← In-memory backend    (depends on: tokio only)
└── buqueue/             ← Facade crate — re-exports everything via feature flags
    └── examples/
        ├── kafka_send_receive.rs
        ├── kafka_consumer_group.rs
        ├── nats_jetstream_workqueue.rs
        ├── nats_jetstream_fanout.rs
        ├── sqs_standard.rs
        ├── sqs_fifo_ordering.rs
        ├── rabbitmq_direct.rs
        ├── rabbitmq_topic_exchange.rs
        ├── redis_streams.rs
        ├── scheduled_delivery.rs
        ├── dlq_redrive.rs
        └── graceful_shutdown.rs
```

---

## Roadmap

**v0.1 — Core + All Five Backends (current focus)**
All backends with send, receive, batch, scheduling, DLQ, routing keys, tracing,
async Stream consumers, and graceful shutdown.

**v0.2 — Observability**
Built-in metrics hooks for Prometheus and OpenTelemetry. Per-queue throughput, latency
histograms, error rates, and DLQ depth instrumentation.

**v0.3 — Azure Service Bus**
Completing the cloud-provider story for Azure-native teams.

**v1.0 — Stable API**
Semver stability guarantee. Full migration guide from all pre-1.0 versions.

---

## License

MIT — see [LICENSE](LICENSE).
