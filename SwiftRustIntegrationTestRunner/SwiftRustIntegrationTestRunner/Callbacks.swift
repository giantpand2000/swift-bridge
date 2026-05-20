//
//  Callbacks.swift
//  SwiftRustIntegrationTestRunner
//
//  Created by Frankie Nwafili on 9/11/22.
//

import Foundation

func swift_takes_fnonce_callback_no_args_no_return(arg: () -> ()) {
    arg()
}

func swift_takes_fnonce_callback_primitive(
    arg: (UInt8) -> UInt8
) -> UInt8 {
    arg(4)
}

func swift_takes_fnonce_callback_opaque_rust(
    arg: (CallbackTestOpaqueRustType) -> CallbackTestOpaqueRustType
) {
    let doubled = arg(CallbackTestOpaqueRustType(10))
    if doubled.val() != 20 {
        fatalError("Callback not called")
    }
}

func swift_takes_two_fnonce_callbacks(
    arg1: () -> (),
    arg2: (UInt8) -> UInt16
) -> UInt16 {
    arg1()
    return arg2(3)
}

func swift_takes_fnonce_callback_with_two_params(
    arg: (UInt8, UInt16) -> UInt16
) -> UInt16 {
    arg(1, 2)
}

func swift_calls_rust_callbacks() {
    var called = false
    rust_takes_callback_fnonce_no_args_no_return {
        called = true
    }
    if !called {
        fatalError("FnOnce callback was not called")
    }

    let doubled = rust_takes_callback_fnonce_primitive { num in
        num * 2
    }
    if doubled != 4 {
        fatalError("FnOnce primitive callback returned \(doubled)")
    }

    let fnTotal = rust_takes_callback_fn_primitive { num in
        num * 2
    }
    if fnTotal != 10 {
        fatalError("Fn primitive callback returned \(fnTotal)")
    }

    let arcFnTotal = rust_takes_callback_arc_fn_primitive { num in
        num + 1
    }
    if arcFnTotal != 7 {
        fatalError("Arc Fn primitive callback returned \(arcFnTotal)")
    }

    let opaqueVal = rust_takes_callback_fnonce_opaque_rust { rustType in
        rustType.double()
        return rustType
    }
    if opaqueVal != 200 {
        fatalError("FnOnce opaque callback returned \(opaqueVal)")
    }

    let twoParamVal = rust_takes_callback_fnonce_two_params { num, rustType in
        UInt32(num) + rustType.val()
    }
    if twoParamVal != 345 {
        fatalError("FnOnce two-param callback returned \(twoParamVal)")
    }

    var firstNoopCalled = false
    var secondNoopCalled = false
    rust_takes_two_callbacks_fnonce_noop({
        firstNoopCalled = true
    }, {
        secondNoopCalled = true
    })
    if !firstNoopCalled || !secondNoopCalled {
        fatalError("FnOnce noop callbacks were not both called")
    }
}

/// When given an FnOnce callback this should panic.
func swift_calls_rust_fnonce_callback_twice(arg: () -> ()) {
    arg()
    arg()
}

class SwiftMethodCallbackTester {
    func method_with_fnonce_callback(callback: () -> ()) {
        callback()
    }
    
    func method_with_fnonce_callback_primitive(callback: (UInt16) -> UInt16) -> UInt16 {
        callback(5)
    }
}
