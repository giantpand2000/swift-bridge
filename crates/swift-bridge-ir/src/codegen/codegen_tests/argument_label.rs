use super::{CodegenTest, ExpectedCHeader, ExpectedRustTokens, ExpectedSwiftCode};
use proc_macro2::TokenStream;
use quote::quote;

/// Verify that we can properly handle `#[swift_bridge(label = "...")]` attributes.
mod argument_label {
    use super::*;

    fn bridge_module_tokens() -> TokenStream {
        quote! {
            #[swift_bridge::bridge]
            mod ffi {
                extern "Rust" {
                    fn some_function(
                        #[swift_bridge(label = "argumentLabel1")] parameter_name1: i32,
                        #[swift_bridge(label = "argumentLabel2")] parameter_name2: u32,
                    );
                }
            }
        }
    }

    fn expected_rust_tokens() -> ExpectedRustTokens {
        ExpectedRustTokens::Contains(quote! {
            fn __swift_bridge__some_function(parameter_name1: i32, parameter_name2: u32) {
                super::some_function(parameter_name1, parameter_name2)
            }
        })
    }

    fn expected_swift_code() -> ExpectedSwiftCode {
        ExpectedSwiftCode::ContainsAfterTrim(
            r#"
public func some_function(argumentLabel1 parameter_name1: Int32, argumentLabel2 parameter_name2: UInt32) {
    __swift_bridge__$some_function(parameter_name1, parameter_name2)
}
            
"#,
        )
    }

    fn expected_c_header() -> ExpectedCHeader {
        ExpectedCHeader::ContainsAfterTrim(
            r#"
void __swift_bridge__$some_function(int32_t parameter_name1, uint32_t parameter_name2);
"#,
        )
    }

    #[test]
    fn argument_label() {
        CodegenTest {
            bridge_module: bridge_module_tokens().into(),
            expected_rust_tokens: expected_rust_tokens(),
            expected_swift_code: expected_swift_code(),
            expected_c_header: expected_c_header(),
        }
        .test();
    }
}

/// Verify that we can properly handle a `#[swift_bridge(label = "...")]` attribute with only one argument corresponding.
mod argument_one_label {
    use super::*;

    fn bridge_module_tokens() -> TokenStream {
        quote! {
            #[swift_bridge::bridge]
            mod ffi {
                extern "Rust" {
                    fn some_function(
                        #[swift_bridge(label = "argumentLabel1")] parameter_name1: i32,
                        parameter_name2: u32,
                    );
                }
            }
        }
    }

    fn expected_rust_tokens() -> ExpectedRustTokens {
        ExpectedRustTokens::Contains(quote! {
            fn __swift_bridge__some_function(parameter_name1: i32, parameter_name2: u32) {
                super::some_function(parameter_name1, parameter_name2)
            }
        })
    }

    fn expected_swift_code() -> ExpectedSwiftCode {
        ExpectedSwiftCode::ContainsAfterTrim(
            r#"
public func some_function(argumentLabel1 parameter_name1: Int32, _ parameter_name2: UInt32) {
    __swift_bridge__$some_function(parameter_name1, parameter_name2)
}

"#,
        )
    }

    fn expected_c_header() -> ExpectedCHeader {
        ExpectedCHeader::ContainsAfterTrim(
            r#"
void __swift_bridge__$some_function(int32_t parameter_name1, uint32_t parameter_name2);
"#,
        )
    }

    #[test]
    fn argument_label() {
        CodegenTest {
            bridge_module: bridge_module_tokens().into(),
            expected_rust_tokens: expected_rust_tokens(),
            expected_swift_code: expected_swift_code(),
            expected_c_header: expected_c_header(),
        }
        .test();
    }
}

/// Verify that extern "Swift" functions with `#[swift_bridge(label = "_")]` generate
/// Swift wrapper code that calls the Swift function without parameter labels.
mod extern_swift_argument_label_underscore {
    use super::*;

    fn bridge_module_tokens() -> TokenStream {
        quote! {
            #[swift_bridge::bridge]
            mod ffi {
                extern "Swift" {
                    fn some_function(
                        #[swift_bridge(label = "_")] arg1: i32,
                        #[swift_bridge(label = "_")] arg2: u32,
                    );
                }
            }
        }
    }

    fn expected_rust_tokens() -> ExpectedRustTokens {
        ExpectedRustTokens::Contains(quote! {
            pub fn some_function(arg1: i32, arg2: u32) {
                unsafe { __swift_bridge__some_function(arg1, arg2) }
            }
        })
    }

    fn expected_swift_code() -> ExpectedSwiftCode {
        ExpectedSwiftCode::ContainsAfterTrim(
            r#"
@_cdecl("__swift_bridge__$some_function")
public func __swift_bridge__some_function (_ arg1: Int32, _ arg2: UInt32) {
    some_function(arg1, arg2)
}
"#,
        )
    }

    fn expected_c_header() -> ExpectedCHeader {
        ExpectedCHeader::ExactAfterTrim(r#""#)
    }

    #[test]
    fn extern_swift_argument_label_underscore() {
        CodegenTest {
            bridge_module: bridge_module_tokens().into(),
            expected_rust_tokens: expected_rust_tokens(),
            expected_swift_code: expected_swift_code(),
            expected_c_header: expected_c_header(),
        }
        .test();
    }
}

/// Verify that extern "Swift" functions with custom `#[swift_bridge(label = "...")]` generate
/// Swift wrapper code that calls the Swift function with the custom labels.
mod extern_swift_argument_label_custom {
    use super::*;

    fn bridge_module_tokens() -> TokenStream {
        quote! {
            #[swift_bridge::bridge]
            mod ffi {
                extern "Swift" {
                    fn some_function(
                        #[swift_bridge(label = "firstArg")] arg1: i32,
                        #[swift_bridge(label = "secondArg")] arg2: u32,
                    );
                }
            }
        }
    }

    fn expected_rust_tokens() -> ExpectedRustTokens {
        ExpectedRustTokens::Contains(quote! {
            pub fn some_function(arg1: i32, arg2: u32) {
                unsafe { __swift_bridge__some_function(arg1, arg2) }
            }
        })
    }

    fn expected_swift_code() -> ExpectedSwiftCode {
        ExpectedSwiftCode::ContainsAfterTrim(
            r#"
@_cdecl("__swift_bridge__$some_function")
public func __swift_bridge__some_function (firstArg arg1: Int32, secondArg arg2: UInt32) {
    some_function(firstArg: arg1, secondArg: arg2)
}
"#,
        )
    }

    fn expected_c_header() -> ExpectedCHeader {
        ExpectedCHeader::ExactAfterTrim(r#""#)
    }

    #[test]
    fn extern_swift_argument_label_custom() {
        CodegenTest {
            bridge_module: bridge_module_tokens().into(),
            expected_rust_tokens: expected_rust_tokens(),
            expected_swift_code: expected_swift_code(),
            expected_c_header: expected_c_header(),
        }
        .test();
    }
}

/// Verify that extern "Swift" functions with mixed labels (some custom, some underscore, some default)
/// generate correct Swift wrapper code.
mod extern_swift_argument_label_mixed {
    use super::*;

    fn bridge_module_tokens() -> TokenStream {
        quote! {
            #[swift_bridge::bridge]
            mod ffi {
                extern "Swift" {
                    fn some_function(
                        #[swift_bridge(label = "_")] arg1: i32,
                        #[swift_bridge(label = "customLabel")] arg2: u32,
                        arg3: i64,
                    );
                }
            }
        }
    }

    fn expected_rust_tokens() -> ExpectedRustTokens {
        ExpectedRustTokens::Contains(quote! {
            pub fn some_function(arg1: i32, arg2: u32, arg3: i64) {
                unsafe { __swift_bridge__some_function(arg1, arg2, arg3) }
            }
        })
    }

    fn expected_swift_code() -> ExpectedSwiftCode {
        ExpectedSwiftCode::ContainsAfterTrim(
            r#"
@_cdecl("__swift_bridge__$some_function")
public func __swift_bridge__some_function (_ arg1: Int32, customLabel arg2: UInt32, _ arg3: Int64) {
    some_function(arg1, customLabel: arg2, arg3: arg3)
}
"#,
        )
    }

    fn expected_c_header() -> ExpectedCHeader {
        ExpectedCHeader::ExactAfterTrim(r#""#)
    }

    #[test]
    fn extern_swift_argument_label_mixed() {
        CodegenTest {
            bridge_module: bridge_module_tokens().into(),
            expected_rust_tokens: expected_rust_tokens(),
            expected_swift_code: expected_swift_code(),
            expected_c_header: expected_c_header(),
        }
        .test();
    }
}

/// Verify that Swift-style `func` declarations in extern "Swift" blocks are normalized into
/// Rust-style generated APIs while preserving Swift argument labels.
mod extern_swift_func_syntax {
    use super::*;

    fn bridge_module_tokens() -> TokenStream {
        quote! {
            #[swift_bridge::bridge]
            mod ffi {
                extern "Swift" {
                    func someFunction(_ arg1: Int32, customLabel arg2: UInt32, arg3: Int64);
                }
            }
        }
    }

    fn expected_rust_tokens() -> ExpectedRustTokens {
        ExpectedRustTokens::Contains(quote! {
            pub fn some_function(arg1: i32, arg2: u32, arg3: i64) {
                unsafe { __swift_bridge__some_function(arg1, arg2, arg3) }
            }
        })
    }

    fn expected_swift_code() -> ExpectedSwiftCode {
        ExpectedSwiftCode::ContainsAfterTrim(
            r#"
@_cdecl("__swift_bridge__$some_function")
public func __swift_bridge__some_function (_ arg1: Int32, customLabel arg2: UInt32, _ arg3: Int64) {
    someFunction(arg1, customLabel: arg2, arg3: arg3)
}
"#,
        )
    }

    fn expected_c_header() -> ExpectedCHeader {
        ExpectedCHeader::ExactAfterTrim(r#""#)
    }

    #[test]
    fn extern_swift_func_syntax() {
        CodegenTest {
            bridge_module: bridge_module_tokens().into(),
            expected_rust_tokens: expected_rust_tokens(),
            expected_swift_code: expected_swift_code(),
            expected_c_header: expected_c_header(),
        }
        .test();
    }
}

/// Verify that Swift-style `func` declarations can use `rust_name` to control the generated
/// Rust API name while still calling the original Swift function name.
mod extern_swift_func_syntax_rust_name {
    use super::*;

    fn bridge_module_tokens() -> TokenStream {
        quote! {
            #[swift_bridge::bridge]
            mod ffi {
                extern "Swift" {
                    #[swift_bridge(rust_name = "call_custom")]
                    func callCustom(_ value: Int32, forKey key: UInt32);
                }
            }
        }
    }

    fn expected_rust_tokens() -> ExpectedRustTokens {
        ExpectedRustTokens::Contains(quote! {
            pub fn call_custom(value: i32, key: u32) {
                unsafe { __swift_bridge__call_custom(value, key) }
            }
        })
    }

    fn expected_swift_code() -> ExpectedSwiftCode {
        ExpectedSwiftCode::ContainsAfterTrim(
            r#"
@_cdecl("__swift_bridge__$call_custom")
public func __swift_bridge__call_custom (_ value: Int32, forKey key: UInt32) {
    callCustom(value, forKey: key)
}
"#,
        )
    }

    fn expected_c_header() -> ExpectedCHeader {
        ExpectedCHeader::ExactAfterTrim(r#""#)
    }

    #[test]
    fn extern_swift_func_syntax_rust_name() {
        CodegenTest {
            bridge_module: bridge_module_tokens().into(),
            expected_rust_tokens: expected_rust_tokens(),
            expected_swift_code: expected_swift_code(),
            expected_c_header: expected_c_header(),
        }
        .test();
    }
}

/// Verify that Swift-style `func!` declarations bind to the sole Swift type in the extern block
/// as instance methods.
mod extern_swift_func_syntax_instance_method {
    use super::*;

    fn bridge_module_tokens() -> TokenStream {
        quote! {
            #[swift_bridge::bridge]
            mod ffi {
                extern "Swift" {
                    type Foo;

                    func!(bar(_ value: Int64));
                }
            }
        }
    }

    fn expected_rust_tokens() -> ExpectedRustTokens {
        ExpectedRustTokens::ContainsMany(vec![
            quote! {
                impl Foo {
                    pub fn bar(&self, value: i64) {
                        unsafe { __swift_bridge__Foo_bar(swift_bridge::PointerToSwiftType(self.0), value) }
                    }
                }
            },
            quote! {
                #[link_name = "__swift_bridge__$Foo$bar"]
                fn __swift_bridge__Foo_bar(this: swift_bridge::PointerToSwiftType, value: i64);
            },
        ])
    }

    fn expected_swift_code() -> ExpectedSwiftCode {
        ExpectedSwiftCode::ContainsAfterTrim(
            r#"
@_cdecl("__swift_bridge__$Foo$bar")
public func __swift_bridge__Foo_bar (_ this: UnsafeMutableRawPointer, _ value: Int64) {
    Unmanaged<Foo>.fromOpaque(this).takeUnretainedValue().bar(value)
}
"#,
        )
    }

    fn expected_c_header() -> ExpectedCHeader {
        ExpectedCHeader::ExactAfterTrim(r#""#)
    }

    #[test]
    fn extern_swift_func_syntax_instance_method() {
        CodegenTest {
            bridge_module: bridge_module_tokens().into(),
            expected_rust_tokens: expected_rust_tokens(),
            expected_swift_code: expected_swift_code(),
            expected_c_header: expected_c_header(),
        }
        .test();
    }
}

/// Verify that `static_func!` binds to the sole Swift type in the extern block as a class method.
mod extern_swift_static_func_syntax {
    use super::*;

    fn bridge_module_tokens() -> TokenStream {
        quote! {
            #[swift_bridge::bridge]
            mod ffi {
                extern "Swift" {
                    type Foo;

                    static_func!(bar(_ value: Int64));
                }
            }
        }
    }

    fn expected_rust_tokens() -> ExpectedRustTokens {
        ExpectedRustTokens::ContainsMany(vec![
            quote! {
                impl Foo {
                    pub fn bar(value: i64) {
                        unsafe { __swift_bridge__Foo_bar(value) }
                    }
                }
            },
            quote! {
                #[link_name = "__swift_bridge__$Foo$bar"]
                fn __swift_bridge__Foo_bar(value: i64);
            },
        ])
    }

    fn expected_swift_code() -> ExpectedSwiftCode {
        ExpectedSwiftCode::ContainsAfterTrim(
            r#"
@_cdecl("__swift_bridge__$Foo$bar")
public func __swift_bridge__Foo_bar (_ value: Int64) {
    Foo.bar(value)
}
"#,
        )
    }

    fn expected_c_header() -> ExpectedCHeader {
        ExpectedCHeader::ExactAfterTrim(r#""#)
    }

    #[test]
    fn extern_swift_static_func_syntax() {
        CodegenTest {
            bridge_module: bridge_module_tokens().into(),
            expected_rust_tokens: expected_rust_tokens(),
            expected_swift_code: expected_swift_code(),
            expected_c_header: expected_c_header(),
        }
        .test();
    }
}

/// Verify that Swift-style `init` declarations generate Rust constructors and Swift initializer
/// wrappers.
mod extern_swift_init_func_syntax {
    use super::*;

    fn bridge_module_tokens() -> TokenStream {
        quote! {
            #[swift_bridge::bridge]
            mod ffi {
                extern "Swift" {
                    type Foo;

                    func!(init(_ value: Int64));
                }
            }
        }
    }

    fn expected_rust_tokens() -> ExpectedRustTokens {
        ExpectedRustTokens::ContainsMany(vec![
            quote! {
                impl Foo {
                    pub fn new(value: i64) -> Foo {
                        unsafe { __swift_bridge__Foo_new(value) }
                    }
                }
            },
            quote! {
                #[link_name = "__swift_bridge__$Foo$new"]
                fn __swift_bridge__Foo_new(value: i64) -> Foo;
            },
        ])
    }

    fn expected_swift_code() -> ExpectedSwiftCode {
        ExpectedSwiftCode::ContainsAfterTrim(
            r#"
@_cdecl("__swift_bridge__$Foo$new")
public func __swift_bridge__Foo_new (_ value: Int64) -> UnsafeMutableRawPointer {
    Unmanaged.passRetained(Foo(value)).toOpaque()
}
"#,
        )
    }

    fn expected_c_header() -> ExpectedCHeader {
        ExpectedCHeader::ExactAfterTrim(r#""#)
    }

    #[test]
    fn extern_swift_init_func_syntax() {
        CodegenTest {
            bridge_module: bridge_module_tokens().into(),
            expected_rust_tokens: expected_rust_tokens(),
            expected_swift_code: expected_swift_code(),
            expected_c_header: expected_c_header(),
        }
        .test();
    }
}
