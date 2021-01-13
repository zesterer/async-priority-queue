// Copyright 2020 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! An async-aware priority queue.

use futures::{
    future::FusedFuture,
    stream::{FusedStream, Stream},
};

use std::{
    iter::FusedIterator,
    collections::{BinaryHeap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{Mutex, Arc, atomic::{AtomicBool, Ordering}},
    task::{Context, Poll, Waker},
};

/// An async-aware priority queue.
pub struct PriorityQueue<T> {
    inner: Mutex<(BinaryHeap<T>, VecDeque<(Waker, Arc<AtomicBool>)>)>,
}

impl<T: Ord> Default for PriorityQueue<T> {
    fn default() -> Self {
        Self {
            inner: Mutex::new((BinaryHeap::new(), VecDeque::new())),
        }
    }
}

impl<T: Ord> PriorityQueue<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pushes an item into the queue. It will be removed in an order consistent with the ordering
    /// of itself relative to other items in the queue at the time of removal.
    pub fn push(&self, item: T) {
        let mut inner = self.inner.lock().unwrap();

        inner.0.push(item);

        if let Some((w, woken)) = inner.1.pop_front() {
            woken.store(true, Ordering::Relaxed);
            Waker::wake(w)
        }
    }

    /// Attempts to remove the item with the highest priority from the queue, returning [`None`] if
    /// there are no available items.
    pub fn try_pop(&self) -> Option<T> {
        self.inner.lock().unwrap().0.pop()
    }

    /// Removes the item with the highest priority from the queue, waiting for an item should there
    /// not be one immediately available.
    ///
    /// Items are removed from the queue on a 'first come, first served' basis.
    pub fn pop(&self) -> impl Future<Output = T> + FusedFuture + '_ {
        PopFut {
            queue: self,
            terminated: false,
            woken: None,
        }
    }

    /// Returns a stream of highest-priority items from this queue.
    ///
    /// Items are removed from the queue on a 'first come, first served' basis.
    pub fn incoming(&self) -> IncomingStream<T> {
        IncomingStream {
            queue: self,
            woken: None,
        }
    }

    /// Returns an iterator of pending items from the queue (i.e: those that have already been inserted). Items will
    /// only be removed from the queue as the iterator is advanced.
    pub fn pending(&self) -> impl Iterator<Item = T> + '_ {
        std::iter::from_fn(move || self.inner.lock().unwrap().0.pop())
    }

    /// Returns an iterator of items currently occupying the queue, immediately draining the queue.
    pub fn drain(&self) -> impl ExactSizeIterator<Item = T> + FusedIterator {
        std::mem::take(&mut self.inner.lock().unwrap().0).into_iter()
    }

    /// Return the number of items currently occupying the priority queue.
    ///
    /// Because the queue is asynchronous, this information should be considered out of date immediately and, as such,
    /// should only be used for the purpose of logging, heuristics, etc.
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().0.len()
    }

    /// Return `true` if the priority queue is currently empty.
    ///
    /// Because the queue is asynchronous, this information should be considered out of date immediately and, as such,
    /// should only be used for the purpose of logging, heuristics, etc.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().0.is_empty()
    }
}

/// A future representing an item to be removed from the priority queue.
pub(crate) struct PopFut<'a, T> {
    queue: &'a PriorityQueue<T>,
    terminated: bool,
    woken: Option<Arc<AtomicBool>>,
}

impl<'a, T: Ord> Future for PopFut<'a, T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let mut inner = self.queue.inner.lock().unwrap();

        match inner.0.pop() {
            _ if self.terminated => Poll::Pending,
            Some(entry) => {
                self.terminated = true;
                self.woken = None;
                Poll::Ready(entry)
            }
            None => {
                let woken = Arc::new(AtomicBool::new(false));
                inner.1.push_back((cx.waker().clone(), woken.clone()));
                self.woken = Some(woken);
                Poll::Pending
            }
        }
    }
}

impl<'a, T: Ord> FusedFuture for PopFut<'a, T> {
    fn is_terminated(&self) -> bool {
        self.terminated
    }
}

impl<'a, T> Drop for PopFut<'a, T> {
    fn drop(&mut self) {
        // We were woken but didn't receive anything, wake up another
        if self.woken.take().map_or(false, |w| w.load(Ordering::Relaxed)) {
            if let Some((w, woken)) = self.queue.inner.lock().unwrap().1.pop_front() {
                woken.store(true, Ordering::Relaxed);
                Waker::wake(w)
            }
        }
    }
}

/// A stream of items removed from the priority queue.
pub struct IncomingStream<'a, T> {
    queue: &'a PriorityQueue<T>,
    woken: Option<Arc<AtomicBool>>,
}

impl<'a, T: Ord> Stream for IncomingStream<'a, T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut inner = self.queue.inner.lock().unwrap();

        match inner.0.pop() {
            Some(entry) => {
                self.woken = None;
                Poll::Ready(Some(entry))
            },
            None => {
                // Attempt to reuse the `Arc` to avoid an allocation, but don't do so at the risk of missing a wakup
                let woken = match self.woken.clone() {
                    Some(mut woken) => match Arc::get_mut(&mut woken) {
                        Some(w) => { *w.get_mut() = false; woken },
                        None => Arc::new(AtomicBool::new(false)),
                    },
                    None => Arc::new(AtomicBool::new(false)),
                };
                inner.1.push_back((cx.waker().clone(), woken.clone()));
                self.woken = Some(woken);
                Poll::Pending
            }
        }
    }
}

impl<'a, T: Ord> FusedStream for IncomingStream<'a, T> {
    fn is_terminated(&self) -> bool {
        false
    }
}

impl<'a, T> Drop for IncomingStream<'a, T> {
    fn drop(&mut self) {
        // We were woken but didn't receive anything, wake up another
        if self.woken.take().map_or(false, |w| w.load(Ordering::Relaxed)) {
            if let Some((w, woken)) = self.queue.inner.lock().unwrap().1.pop_front() {
                woken.store(true, Ordering::Relaxed);
                Waker::wake(w)
            }
        }
    }
}
