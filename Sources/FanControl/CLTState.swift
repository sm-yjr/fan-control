import Combine
import SwiftUI

/// A SwiftUI-compatible local state container that can be built with the
/// standalone Command Line Tools. Recent macOS SDKs declare SwiftUI.State as
/// an external macro, but CLT does not ship the SwiftUIMacros plugin.
@propertyWrapper
struct CLTState<Value>: DynamicProperty {
    private final class Storage: ObservableObject {
        @Published var value: Value

        init(_ value: Value) {
            self.value = value
        }
    }

    @StateObject private var storage: Storage

    init(wrappedValue: Value) {
        _storage = StateObject(wrappedValue: Storage(wrappedValue))
    }

    init(initialValue: Value) {
        _storage = StateObject(wrappedValue: Storage(initialValue))
    }

    var wrappedValue: Value {
        get { storage.value }
        nonmutating set { storage.value = newValue }
    }

    var projectedValue: Binding<Value> {
        Binding(
            get: { storage.value },
            set: { storage.value = $0 }
        )
    }
}
