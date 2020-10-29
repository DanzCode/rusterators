use std::mem::{transmute, take};

use context::{Transfer, Context, ContextFn};

use crate::utils::SelfUpdating;
use context::stack::{ProtectedFixedSizeStack};

pub struct StackFactory(Box<dyn FnOnce()->ProtectedFixedSizeStack>);

impl StackFactory {
    fn new<F:FnOnce()->ProtectedFixedSizeStack+'static>(builder:F) -> Self {
        Self(Box::new(builder))
    }

    pub fn default_stack() -> Self {
        Self::new(|| ProtectedFixedSizeStack::default())
    }

    pub fn of_size(stack_size:usize) -> Self {
        Self::new(move || ProtectedFixedSizeStack::new(stack_size).unwrap())
    }

    pub fn build(self) -> ProtectedFixedSizeStack {
        (self.0)()
    }
}

/// Container technically quite simular to Option but with special purpose to hold a value that can be moved out exactly once (also semanticly)
/// It is thought to move data between two callstacks by having a known mutable reference for this container where the value is passed to before execution control is switched
/// Resuming execution can than move the value by returning it from yield/suspense call leaving the container at the "swap place" being emtpy variant
#[derive(Debug)]
enum ValueExchangeContainer<V> {
    Value(V),
    Empty,
}

impl<V> From<V> for ValueExchangeContainer<V> {
    fn from(v: V) -> Self {
        ValueExchangeContainer::prepare_exchange(v)
    }
}

impl<V> Default for ValueExchangeContainer<V> {
    /// Defining Empty variant as default value
    /// Needed to use std::mem::take
    fn default() -> Self {
        Self::Empty
    }
}

impl<V> ValueExchangeContainer<V> {
    /// Wrap a value V in a ValueExchangeContainer
    fn prepare_exchange(val: V) -> Self {
        Self::Value(val)
    }

    /// Queries whether containers value is still available or has already been moved
    fn has_content(&self) -> bool {
        match self {
            Self::Value(_) => true,
            Self::Empty => false
        }
    }
    /// Move value out of container by returning the value and changing containers value to variant empty
    /// Panics if container is already empty
    fn receive_content(&mut self) -> V {
        match take(self) {
            Self::Value(v) => v,
            Self::Empty => panic!("No content to receive")
        }
    }
    /// Encodes a reference to this container as usize for transfer
    fn make_pointer(&self) -> usize {
        unsafe { transmute::<*const Self, usize>(self as *const Self) }
    }
    /// Reconstructs a mutable reference to a Container from a usize pointer
    fn of_pointer<'a>(p: usize) -> &'a mut Self {
        unsafe {
            &mut *transmute::<usize, *mut Self>(p)
        }
    }
}

/// Decorator around a mutable Container ref providing trans-callcontext access
/// While ValueExchangeContainer itself is kind of "immutable", i.e. should only used for one move and each value should be packed in a fresh instance
/// the ref is thought to be mutable and reference may be updated
struct ExchangeContainerRef<'a, V>(&'a mut ValueExchangeContainer<V>);

impl<'a, V> ExchangeContainerRef<'a, V> {
    /// create from mutable reference (e.g. by ValueExchangeContainer::of_pointer)
    fn new(container: &'a mut ValueExchangeContainer<V>) -> Self {
        Self(container)
    }
    /// create from usize pointer
    fn of_pointer(p: usize) -> Self {
        Self::new(ValueExchangeContainer::of_pointer(p))
    }

    /// Sends a value into the referenced Container
    /// Either writes a new created container to the reference or panics if given container is not empty
    fn send_value(&mut self, val: V) {
        match self.0 {
            ValueExchangeContainer::Value(_) => panic!("tried to write to non-empty container"),
            ValueExchangeContainer::Empty => { *self.0 = ValueExchangeContainer::prepare_exchange(val); }
        }
    }
    /// Updates the holded reference to new pointer
    fn receive_ref(&mut self, p: usize) {
        self.0 = match self.0 {
            ValueExchangeContainer::Empty => ValueExchangeContainer::of_pointer(p),
            _ => panic!("tried to forget nonm-empty container ref")
        };
    }
}

impl<'a, V> From<usize> for ExchangeContainerRef<'a, V> {
    fn from(p: usize) -> Self {
        Self::of_pointer(p)
    }
}

/// Wraps the context libs raw transfer type which allows to exchange pointer adding the possibility to move input and output values between callstacks
/// Therefore it has two additional attributes:
/// - one field allocating a ValueExchangeContainer in which another context may transfer input values of type ReceiveMessage
/// - one containing an optional ContainerRef to the receiving field of an ExchangingTransfer in the opposite context in which output values can be moved
///
/// The interface only offers complete control cycle methods (maybe send data -> switch context and wait for resume -> read received data) and encapsulates this behaviour on the lowest possible level
pub struct ExchangingTransfer<'a, SendMessage, ReceiveMessage> {
    pointer_transfer: SelfUpdating<Transfer>,
    receive_container: ValueExchangeContainer<ReceiveMessage>,
    send_ref: Option<ExchangeContainerRef<'a, SendMessage>>,
}

impl<'a, Send, Receive> ExchangingTransfer<'a, Send, Receive> {
    /// Creates an ExchangingTransfer out of a raw transfer which pointer does not belong to another ExchangeContainer reference
    /// In this case no output can be send on first suspense, since the destination is unknown and therefore only suspense call(which does not send) is valid
    pub(super) fn create_without_send(pointer_transfer: Transfer) -> Self {
        Self {
            pointer_transfer: pointer_transfer.into(),
            receive_container: ValueExchangeContainer::default(),
            send_ref: None,
        }
    }
    /// Creates an ExchangingTransfer by a raw transfer already containing a valid ref to another ExchangeContainer
    /// This instance will be able to send output on first suspense (and might have to, depending on higher level semantics)
    pub(super) fn create_with_send(pointer_transfer: Transfer) -> Self {
        let current_data = pointer_transfer.data;
        Self {
            pointer_transfer: pointer_transfer.into(),
            receive_container: ValueExchangeContainer::default(),
            send_ref: Some(ExchangeContainerRef::of_pointer(current_data)),
        }
    }

    /// Creates an ExchangingTransfer out of a raw transfer using the initial transfer pointer to resolve a different value and there creates an ExTansfer without sending capability on first suspense (see create_without_send)
    pub(super) fn create_receiving<V>(pointer_transfer: Transfer) -> (Self, V) {
        let receive = ValueExchangeContainer::of_pointer(pointer_transfer.data).receive_content();
        (Self::create_without_send(pointer_transfer), receive)
    }

    /// Creates an ExchangingTransfer by creating a raw transfer first on top of a stack builded by given [stack_factory] pointing to  [context_fn]
    /// Transfers [initial] using pointer to ValueExchangeContainer and suspends execution control to created context
    /// Returns tupel of created ExchangingTransfer and builded stack after resume
    pub(super) fn init_context_sending<V>(stack_factory:StackFactory,context_fn:ContextFn,initial:V) -> (Self, ProtectedFixedSizeStack) {
        let stack=stack_factory.build();
        let transfer=unsafe {
            Transfer::new(Context::new(&stack, context_fn), 0)
                .context.resume(ValueExchangeContainer::prepare_exchange(initial).make_pointer())
        };
        (Self::create_with_send(transfer), stack)
    }

    /// Sends given value [val] to connected callcontext and resumes it's execution expecting to never come back
    /// Therefore a nullpointer is transferred for current Input ExchangeContainer reference (as no input should occur ever again)
    /// Panics if this context is resumed ever again
    pub(super) fn dispose_with(&mut self, val: Send) -> ! {
        self.send(val);
        self.pointer_transfer.update(|t| unsafe { t.context.resume(0) });
        panic!("resumed after dispose");
    }

    /// Sends given value [val] to connected callcontext and resumes it's execution expecting that current callcontext is resumed later
    /// Therefore a reference to the current input container field is send as pointer and - after resuming - expects this container to be filled and therefore returns it's content
    /// Panics if no ref for output is known or input container is empty after resume
    pub(super) fn yield_with(&mut self, val: Send) -> Receive {
        self.send(val);
        self.suspend()
    }

    /// Writes [val] to current ExchangeContainerRef or panics in case the ref is unknown
    fn send(&mut self, val: Send) {
        match &mut self.send_ref {
            Some(send_ref) => send_ref.send_value(val),
            None => panic!("invalid exchange state for sending")
        };
    }
    /// like [yield_with] but without sending a value
    pub(super) fn suspend(&mut self) -> Receive {
        let receive_container_pointer = self.receive_container.make_pointer();
        self.pointer_transfer.update(|t| unsafe { t.context.resume(receive_container_pointer) });
        if self.pointer_transfer.data != 0 {
            self.send_ref = Some(self.send_ref.take().map(|mut s| {
                s.receive_ref(self.pointer_transfer.data);
                s
            }).unwrap_or_else(|| ExchangeContainerRef::of_pointer(self.pointer_transfer.data)));
        } else {
            self.send_ref = None;
        }
        self.receive_container.receive_content()
    }
}
#[cfg(test)]
mod tests {
    use context::{Context, ContextFn, Transfer};
    use context::stack::ProtectedFixedSizeStack;
    use super::ValueExchangeContainer;
    use crate::transfer::{ExchangeContainerRef, ExchangingTransfer};
    use std::panic::{catch_unwind, AssertUnwindSafe};

    #[test]
    fn exchange_container_prepare() {
        let container = ValueExchangeContainer::prepare_exchange(1);
        if let ValueExchangeContainer::Value(content) = container {
            assert_eq!(content, 1)
        } else {
            panic!("value should exists")
        }
    }

    #[test]
    fn exchange_container_has_content_correct_result() {
        let container = ValueExchangeContainer::prepare_exchange(1);
        assert_eq!(container.has_content(), true);
        let container = ValueExchangeContainer::<usize>::Empty;
        assert_eq!(container.has_content(), false);
    }

    #[test]
    fn exchange_container_receive_content() {
        let mut container = ValueExchangeContainer::prepare_exchange(1);
        assert_eq!(container.receive_content(), 1);
        assert_eq!(container.has_content(), false);
    }

    #[test]
    fn exchange_container_dup_by_pointer() {
        let exchange_container = ValueExchangeContainer::prepare_exchange(1);
        let dup_container = ValueExchangeContainer::<i32>::of_pointer(exchange_container.make_pointer());
        assert_eq!(dup_container.receive_content(), 1)
    }

    #[test]
    fn exchange_ref_new() {
        let mut container = ValueExchangeContainer::prepare_exchange(1);
        let container_ref = ExchangeContainerRef::new(&mut container);
        assert_eq!(container_ref.0.receive_content(), 1);
        let mut container = ValueExchangeContainer::<i32>::Empty;
        let container_ref = ExchangeContainerRef::new(&mut container);
        assert_eq!(container_ref.0.has_content(), false);
    }

    #[test]
    fn exchange_ref_of_pointer() {
        let container = ValueExchangeContainer::prepare_exchange(1);
        let container_ref = ExchangeContainerRef::<i32>::of_pointer(container.make_pointer());
        assert_eq!(container_ref.0.receive_content(), 1);
        let container = ValueExchangeContainer::<i32>::Empty;
        let container_ref = ExchangeContainerRef::<i32>::of_pointer(container.make_pointer());
        assert_eq!(container_ref.0.has_content(), false)
    }

    #[test]
    fn exchange_ref_send() {
        let mut container = ValueExchangeContainer::<i32>::Empty;
        let mut container_ref = ExchangeContainerRef::new(&mut container);
        container_ref.send_value(2);
        assert_eq!(container.receive_content(), 2)
    }

    #[test]
    fn exchange_ref_receive() {
        let mut container = ValueExchangeContainer::<i32>::Empty;
        let mut container_ref = ExchangeContainerRef::new(&mut container);
        let alt_container = ValueExchangeContainer::prepare_exchange(3);
        container_ref.receive_ref(alt_container.make_pointer());
        assert_eq!(container_ref.0.receive_content(), 3)
    }


    static mut STATIC_TEST_STACK: Option<ProtectedFixedSizeStack> = None;

    fn create_test_context(test_fn: ContextFn, start_data: usize) -> Transfer {
        unsafe {
            STATIC_TEST_STACK = Some(ProtectedFixedSizeStack::default())
        }
        unsafe { Transfer::new(Context::new(STATIC_TEST_STACK.as_ref().unwrap(), test_fn), start_data) }
    }

    extern "C" fn init_test(_: Transfer) -> ! {
        panic!("")
    }

    #[test]
    fn transfer_create_without_send() {
        let test_transfer = create_test_context(init_test, 0);
        let transfer = ExchangingTransfer::<i32, i32>::create_without_send(test_transfer);
        assert_eq!(transfer.pointer_transfer.data, 0);
        assert!(!transfer.receive_container.has_content());
        assert!(transfer.send_ref.is_none())
    }

    #[test]
    fn transfer_create_with_send() {
        let test_exchange = ValueExchangeContainer::prepare_exchange(5);
        let test_transfer = create_test_context(init_test, test_exchange.make_pointer());
        let transfer = ExchangingTransfer::<i32, i32>::create_with_send(test_transfer);
        assert_eq!(transfer.pointer_transfer.data, test_exchange.make_pointer());
        assert!(!transfer.receive_container.has_content());
        assert_eq!(transfer.send_ref.unwrap().0.receive_content(), 5)
    }

    #[test]
    fn transfer_create_receiving() {
        let test_exchange = ValueExchangeContainer::prepare_exchange("test");
        let test_transfer = create_test_context(init_test, test_exchange.make_pointer());
        let (transfer, initial) = ExchangingTransfer::<i32, i32>::create_receiving::<&str>(test_transfer);
        assert_eq!(transfer.pointer_transfer.data, test_exchange.make_pointer());
        assert!(!transfer.receive_container.has_content());
        assert_eq!(transfer.send_ref.is_none(), true);
        assert_eq!(initial, "test")
    }

    #[test]
    fn transfer_dispose_with() {
        extern "C" fn dispose_test(t: Transfer) -> ! {
            let mut trans = ExchangingTransfer::<i32, i32>::create_with_send(t);
            trans.dispose_with(3)
        }
        let mut test_exchange = ValueExchangeContainer::<i32>::Empty;
        unsafe { create_test_context(dispose_test, 0).context.resume(test_exchange.make_pointer()) };
        assert_eq!(test_exchange.receive_content(), 3)
    }

    #[test]
    fn transfer_yield_with() {
        extern "C" fn dispose_test(t: Transfer) -> ! {
            let mut trans = ExchangingTransfer::<i32, i32>::create_with_send(t);
            trans.yield_with(2);
            trans.dispose_with(0)
        }
        let mut test_exchange = ValueExchangeContainer::<i32>::Empty;
        let t = unsafe { create_test_context(dispose_test, 0).context.resume(test_exchange.make_pointer()) };
        assert_eq!(test_exchange.receive_content(), 2);
        ExchangeContainerRef::of_pointer(t.data).send_value(1);
        unsafe { t.context.resume(test_exchange.make_pointer()) };
        assert_eq!(test_exchange.receive_content(), 0);
    }

    #[test]
    #[should_panic]
    fn transfer_dispose_with_does_not_allow_resume() {
        extern "C" fn dispose_test(t: Transfer) -> ! {
            unsafe { t.context.resume(0) };
            panic!()
        }
        let test_exchange = ValueExchangeContainer::<i32>::Empty;
        let mut t = ExchangingTransfer::<i32, i32>::create_with_send(create_test_context(dispose_test, test_exchange.make_pointer()));
        t.dispose_with(5);
    }
}