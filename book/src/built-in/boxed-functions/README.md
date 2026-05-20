# Boxed Functions

## Box<dyn FnOnce(A, B) -> C>

`swift-bridge` supports bridging boxed `FnOnce` functions with any number of arguments
in both directions.

There is a panic if you attempt to call a bridged `FnOnce` function more than once.

```rust
#[swift_bridge::bridge]
mod ffi {
	extern "Swift" {
	    type CreditCardReader;
	    type Card;
	    type CardError;

        fn processCard(
            self: &CreditCardReader,
            callback: Box<dyn FnOnce(Result<Card, CardError>) -> ()>
        );
	}
}
```

## Box<dyn Fn(A, B) -> C>

`Box<dyn Fn(A, B) -> C>` is supported in both directions for repeatable callbacks.

## Arc<dyn Fn(A, B) -> C>

`Arc<dyn Fn(A, B) -> C>` is supported for callbacks that need shared Rust ownership.
`Arc<dyn FnOnce(A, B) -> C>` is not supported because `FnOnce` must be consumed when
called, which conflicts with `Arc` shared ownership.
