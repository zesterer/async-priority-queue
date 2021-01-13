# `async-priority-queue`

[![crates.io](https://img.shields.io/crates/v/async-priority-queue.svg)](https://crates.io/crates/async-priority-queue)
[![crates.io](https://docs.rs/async-priority-queue/badge.svg)](https://docs.rs/async-priority-queue)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/zesterer/async-priority-queue)

An async-aware priority queue.

## Example

```rust
let queue = PriorityQueue::new();

queue.push(3);
queue.push(1);
queue.push(2);

assert_eq!(queue.pop().await, 3);
assert_eq!(queue.pop().await, 2);
assert_eq!(queue.pop().await, 1);
```

## License

I originally wrote this crate during employment by IOTA Stiftung. IOTA still legally owns the code, but it was licensed
under Apache 2.0, meaning that I have the right to modify and redistribute it under my own name.

`async-priority-queue` is distributed under the
[Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0) (see `LICENSE`).
