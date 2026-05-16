#[swift_bridge::bridge]
mod ffi {
    extern "Swift" {
        #[swift_bridge(rust_name = "call_custom")]
        func!(callCustom(_ value: Int32, forKey key: UInt32) -> UInt32);

        func!(loadURL(_ id: Int32));
    }
}

#[swift_bridge::bridge]
mod typed_ffi {
    extern "Swift" {
        type Foo;

        func!(bar(_ value: Int64));
        static_func!(baz());
    }
}

fn _assert_generated_api() {
    let _: fn(i32, u32) -> u32 = ffi::call_custom;
    let _: fn(i32) = ffi::load_url;
    let _: fn(&typed_ffi::Foo, i64) = typed_ffi::Foo::bar;
    let _: fn() = typed_ffi::Foo::baz;
}

fn main() {}
