#[swift_bridge::bridge]
mod ffi {
    extern "Swift" {
        #[swift_bridge(rust_name = "call_custom")]
        func!(callCustom(_ value: i32, forKey key: u32) -> u32);

        func!(loadURL(_ id: i32));
    }
}

fn _assert_generated_api() {
    let _: fn(i32, u32) -> u32 = ffi::call_custom;
    let _: fn(i32) = ffi::load_url;
}

fn main() {}
