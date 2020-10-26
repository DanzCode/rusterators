use std::mem::{transmute, replace, take};

use context::Transfer;

use crate::utils::SelfUpdating;

pub enum ValueExchangeContainer<V> {
    Value(V),
    Empty,
}

impl<V> From<V> for ValueExchangeContainer<V> {
    fn from(v: V) -> Self {
        ValueExchangeContainer::prepare_exchange(v)
    }
}

impl<V> Default for ValueExchangeContainer<V> {
    fn default() -> Self {
        Self::Empty
    }
}

impl<V> ValueExchangeContainer<V> {
    pub fn prepare_exchange(val: V) -> Self {
        Self::Value(val)
    }

    pub fn has_content(&self) -> bool {
        match self {
            Self::Value(_) => true,
            Self::Empty => false
        }
    }

    pub fn receive_content(&mut self) -> V {
        match take(self) {
            Self::Value(v) => v,
            Self::Empty => panic!("No content to receive")
        }
    }

    pub(super) fn make_pointer(&self) -> usize {
        unsafe { transmute::<*const Self, usize>(self as *const Self) }
    }

    fn of_pointer<'a>(p: usize) -> &'a mut Self {
        unsafe {
            &mut *transmute::<usize, *mut Self>(p)
        }
    }
}

pub struct ExchangeContainerRef<'a, V>(&'a mut ValueExchangeContainer<V>);

impl<'a, V> ExchangeContainerRef<'a, V> {
    pub fn new(container: &'a mut ValueExchangeContainer<V>) -> Self {
        Self(container)
    }

    pub fn of_pointer(p: usize) -> Self {
        Self::new(ValueExchangeContainer::of_pointer(p))
    }

    pub fn send_value(&mut self, val: V) {
        match self.0 {
            ValueExchangeContainer::Value(_) => panic!("tried to write to non-empty container"),
            ValueExchangeContainer::Empty => { *self.0 = ValueExchangeContainer::prepare_exchange(val); }
        }
    }

    pub fn receive_ref(&mut self, p: usize) {
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

pub struct ExchangingTransfer<'a, SendMessage, ReceiveMessage> {
    pointer_transfer: SelfUpdating<Transfer>,
    receive_container: ValueExchangeContainer<ReceiveMessage>,
    send_ref: Option<ExchangeContainerRef<'a, SendMessage>>,
}

impl<'a, Send, Receive> ExchangingTransfer<'a, Send, Receive> {
    pub fn create_without_send(pointer_transfer: Transfer) -> Self {
        Self {
            pointer_transfer: pointer_transfer.into(),
            receive_container: ValueExchangeContainer::default(),
            send_ref: None,
        }
    }

    pub fn create_with_send(pointer_transfer: Transfer) -> Self {
        let current_data = pointer_transfer.data;
        Self {
            pointer_transfer: pointer_transfer.into(),
            receive_container: ValueExchangeContainer::default(),
            send_ref: Some(ExchangeContainerRef::of_pointer(current_data)),
        }
    }

    pub fn create_receiving<V>(pointer_transfer: Transfer) -> (Self, V) {
        let receive = ValueExchangeContainer::of_pointer(pointer_transfer.data).receive_content();
        (Self::create_without_send(pointer_transfer), receive)
    }

    pub fn dispose_with(&mut self, val: Send) -> ! {
        self.send(val);
        self.pointer_transfer.update(|t| unsafe { t.context.resume(0) });
        panic!("resumed after dispose")
    }

    pub fn yield_with(&mut self, val: Send) -> Receive {
        self.send(val);
        let t = self.suspend();
        t
    }

    fn send(&mut self, val: Send) {
        match &mut self.send_ref {
            Some(send_ref) => send_ref.send_value(val),
            None => panic!("invalid exchange state for sending")
        };
    }

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
        let tmp = self.receive_container.receive_content();
        tmp
    }
}

mod tests {
    use context::{Context, ContextFn, Transfer};
    use context::stack::ProtectedFixedSizeStack;
    use crate::transfer::ValueExchangeContainer;

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
}