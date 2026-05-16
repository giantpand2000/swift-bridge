# extern "Swift"

Use an `extern "Swift"` block to declare Swift types and functions that Rust
will call through the generated bridge.

```rust
#[swift_bridge::bridge]
mod ffi {
    extern "Swift" {
        type AudioEngine;

        func!(setVolume(_ value: UInt32, forChannel channel: UInt32));
        func!(loadURL(_ urlID: UInt32) -> Bool);
    }
}

fn configure() {
    ffi::set_volume(80, 2);

    if ffi::load_url(42) {
        // ...
    }
}
```

The Swift implementation keeps its Swift spelling and labels:

```swift
func setVolume(_ value: UInt32, forChannel channel: UInt32) {
    // ...
}

func loadURL(_ urlID: UInt32) -> Bool {
    // ...
}
```

## Swift-style function syntax

Swift-style declarations are written with `func!(...)` inside `extern "Swift"`.
The macro-like spelling is intentional: Rust must accept the bridge module's
tokens before `swift-bridge` can normalize the declaration into a Rust foreign
function signature.

```rust
#[swift_bridge::bridge]
mod ffi {
    extern "Swift" {
        func!(setValue(_ value: Int32, forKey key: UInt32, limit: UInt32));
    }
}
```

This generates a Rust API shaped like:

```rust
ffi::set_value(value, key, limit);
```

and calls Swift as:

```swift
setValue(value, forKey: key, limit: limit)
```

Parameter labels follow Swift's spelling:

- `_ value: T` omits the Swift argument label and creates a Rust parameter named
  `value`.
- `forKey key: T` uses `forKey` as the Swift argument label and creates a Rust
  parameter named `key`.
- `limit: T` uses the same name for the Swift label and the Rust parameter.

Function names and parameter names are converted to Rust `snake_case`.
For example, `loadURL(_ urlID: UInt32)` becomes `ffi::load_url(url_id)`.

Type names in Swift-style declarations can use either `swift-bridge`'s
Rust-side spelling or the supported Swift-side spelling. Swift spellings are
normalized before the bridge module is parsed as Rust.

| Swift-style spelling | Rust spelling |
| --- | --- |
| `UInt8`, `Int8`, `UInt16`, `Int16`, `UInt32`, `Int32`, `UInt64`, `Int64` | `u8`, `i8`, `u16`, `i16`, `u32`, `i32`, `u64`, `i64` |
| `UInt`, `Int` | `usize`, `isize` |
| `Float`, `Double`, `Bool`, `Void` | `f32`, `f64`, `bool`, `()` |
| `Optional<T>` | `Option<T>` |
| `RustVec<T>` | `Vec<T>` |
| `RustResult<T, E>` | `Result<T, E>` |
| `UnsafePointer<T>`, `UnsafeMutablePointer<T>` | `*const T`, `*mut T` |
| `UnsafeRawPointer`, `UnsafeMutableRawPointer` | `*const std::ffi::c_void`, `*mut std::ffi::c_void` |
| `RustString`, `RustStringRef`, `RustStringRefMut`, `RustStr` | `String`, `&String`, `&mut String`, `&str` |

Plain `String` is also accepted and keeps the existing `swift-bridge` string
behavior.

## Naming overrides

If two Swift names produce the same Rust `snake_case` name, `swift-bridge`
reports an error and asks for an explicit Rust name.

```rust
#[swift_bridge::bridge]
mod ffi {
    extern "Swift" {
        func!(loadURL(_ id: UInt32) -> Bool);

        #[swift_bridge(rust_name = "load_url_from_string")]
        func!(loadUrl(_ id: UInt32) -> Bool);
    }
}
```

`#[swift_bridge(rust_name = "...")]` changes the generated Rust API name while
the Swift call still uses the Swift function name from the `func!(...)`
declaration. You usually do not need `#[swift_bridge(swift_name = "...")]` with
Swift-style declarations because `swift-bridge` records that name for you.

Other function attributes can be placed before `func!(...)` in the same way they
are placed before `fn` declarations.

## Async functions

Swift-style declarations can include `async`:

```rust
#[swift_bridge::bridge]
mod ffi {
    extern "Swift" {
        func!(async fetchUserCount() -> UInt32);
        func!(fetchIsEnabled() async -> Bool);
    }
}
```

## Rust-style function syntax

The existing Rust-style `fn` form remains supported. It is equivalent, but you
must write the Swift name and argument labels with attributes.

```rust
#[swift_bridge::bridge]
mod ffi {
    extern "Swift" {
        #[swift_bridge(swift_name = "setValue")]
        fn set_value(
            #[swift_bridge(label = "_")] value: i32,
            #[swift_bridge(label = "forKey")] key: u32,
            limit: u32,
        );
    }
}
```
