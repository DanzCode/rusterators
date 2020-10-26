/**
*
*/
pub mod transfer {
    use std::mem::{take, swap, replace};
    use context::Transfer;
    use std::intrinsics::transmute;
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

        pub (super) fn make_pointer(&self) -> usize {
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
                ValueExchangeContainer::Empty => {replace(self.0, ValueExchangeContainer::prepare_exchange(val));}
            }
        }

        pub fn receive_ref(&mut self, p: usize) {
            self.0 = match self.0 {
                ValueExchangeContainer::Empty => ValueExchangeContainer::of_pointer(p),
                _ => panic!("tried to forget nonm-empty container ref")
            }
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

        pub fn create_receiving<V>(pointer_transfer:Transfer) -> (Self,V) {
            let receive=ValueExchangeContainer::of_pointer(pointer_transfer.data).receive_content();
            (Self::create_without_send(pointer_transfer),receive)
        }

        pub fn is_sendable(&self) -> bool {
            match self.send_ref {
                None => false,
                Some(_) => true
            }
        }

        pub fn dispose_with(&mut self, val: Send) -> ! {
            self.send(val);
             self.pointer_transfer.update(|t| unsafe {t.context.resume(0) });
            panic!("resumed after dispose")
        }

        pub fn yield_with(&mut self, val: Send) -> Receive {
            self.send(val);
            self.suspend()
        }

        fn send(&mut self, val: Send) {
            match &mut self.send_ref {
                Some(send_ref) => send_ref.send_value(val),
                None => panic!("invalid exchange state for sending")
            }
        }

        pub (super) fn suspend(&mut self) -> Receive {
            let receive_container_pointer = self.receive_container.make_pointer();
            self.pointer_transfer.update(|t| unsafe { t.context.resume(receive_container_pointer) });
            if self.pointer_transfer.data != 0 {
                self.send_ref=Some(self.send_ref.take().map(|mut s| {s.receive_ref(self.pointer_transfer.data);s}).unwrap_or_else(|| ExchangeContainerRef::of_pointer(self.pointer_transfer.data)));
            } else {
                self.send_ref = None;
            }
            self.receive_container.receive_content()
        }
    }

    pub struct ValueMoveTransfer {
        raw_transfer: SelfUpdating<Transfer>
    }

    impl ValueMoveTransfer {
        pub fn new(raw_transfer: Transfer) -> Self {
            Self { raw_transfer: SelfUpdating::of(raw_transfer) }
        }

        pub fn move_transfer_in<V>(&self) -> V {
            if self.raw_transfer.data == 0 {
                panic!("tried to read nullpointer from transfer")
            } else {
                ValueExchangeContainer::<V>::of_pointer(self.raw_transfer.data).receive_content()
            }
        }

        pub fn send_content<V>(&mut self, content: V) {
            let exchange_container = ValueExchangeContainer::prepare_exchange(content);
            self.raw_transfer.update(|t| unsafe { t.context.resume(exchange_container.make_pointer()) });
        }
    }

    pub struct ReceivableValueTransfer(ValueMoveTransfer);

    pub struct SuspendableValueTransfer(ValueMoveTransfer);

    impl ReceivableValueTransfer {
        pub fn init(raw_transfer: Transfer) -> Self {
            Self(ValueMoveTransfer::new(raw_transfer))
        }

        pub fn receive<V>(self) -> (SuspendableValueTransfer, V) {
            let received_content = self.0.move_transfer_in();
            (SuspendableValueTransfer(self.0), received_content)
        }
    }

    impl SuspendableValueTransfer {
        pub fn init(raw_transfer: Transfer) -> Self {
            Self(ValueMoveTransfer::new(raw_transfer))
        }

        pub fn suspend<V>(mut self, content: V) -> ReceivableValueTransfer {
            self.0.send_content(content);
            ReceivableValueTransfer(self.0)
        }
    }

    mod tests {
        use crate::coroutines::transfer::{ValueExchangeContainer, ValueMoveTransfer, ReceivableValueTransfer, SuspendableValueTransfer};
        use context::stack::ProtectedFixedSizeStack;
        use context::{ContextFn, Transfer, Context};

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

        #[test]
        fn value_transfer_init() {
            let test_transfer = create_test_context(init_test, 2);
            let value_transfer = ValueMoveTransfer::new(test_transfer);
            assert_eq!(value_transfer.raw_transfer.data, 2)
        }

        #[test]
        fn value_transfer_receive() {
            let test_container = ValueExchangeContainer::prepare_exchange(2);
            let test_transfer = create_test_context(init_test, test_container.make_pointer());
            let value_transfer = ValueMoveTransfer::new(test_transfer);

            assert_eq!(value_transfer.move_transfer_in::<i32>(), 2)
        }

        #[test]
        fn value_transfer_send() {
            extern "C" fn send_test(t: Transfer) -> ! {
                assert_eq!(ValueExchangeContainer::<i32>::of_pointer(t.data).receive_content(), 2);
                unsafe { t.context.resume(0); }
                panic!()
            }
            let test_transfer = create_test_context(send_test, 0);
            let mut value_transfer = ValueMoveTransfer::new(test_transfer);
            value_transfer.send_content::<i32>(2);
        }

        #[test]
        fn receive_suspendable_transfer_cycle() {
            extern "C" fn send_test(t: Transfer) -> ! {
                let receive_transfer = ReceivableValueTransfer::init(t);
                let (suspend_transfer, rec) = receive_transfer.receive::<i32>();
                assert_eq!(rec, 2);
                suspend_transfer.suspend(3);
                panic!()
            }
            let test_transfer = SuspendableValueTransfer::init(create_test_context(send_test, 0));
            let receive_transfer = test_transfer.suspend(2);
            let (_, rec) = receive_transfer.receive::<i32>();
            assert_eq!(rec, 3)
        }
    }
}

pub mod execution {
    use std::any::Any;
    use crate::utils::SelfUpdating;
    use crate::coroutines::transfer::{SuspendableValueTransfer, ReceivableValueTransfer, ValueMoveTransfer, ValueExchangeContainer, ExchangingTransfer};
    use std::marker::PhantomData;
    use context::{Transfer, Context};
    use std::panic::{catch_unwind, AssertUnwindSafe, resume_unwind};
    use context::stack::ProtectedFixedSizeStack;


    type PanicData = Box<dyn Any + Send + 'static>;

    #[derive(Debug)]
    pub enum UnwindReason {
        Panic(PanicData),
        Drop,
    }

    #[derive(Debug)]
    pub enum CompleteType<Return> {
        Return(Return),
        Unwind(UnwindReason),
    }

    #[derive(Debug)]
    pub enum SuspenseType<Yield, Return> {
        Yield(Yield),
        Complete(CompleteType<Return>),
    }

    #[derive(Debug)]
    pub enum ResumeType<Receive> {
        Yield(Receive),
        Drop(),
    }


    pub struct CoroutineFactory<Yield, Return, Receive, F: Fn(&mut CoroutineChannel<Yield, Return, Receive>, Receive) -> Return>(F, PhantomData<(Yield, Return, Receive)>);

    impl<Yield, Return, Receive, F: Fn(&mut CoroutineChannel<Yield, Return, Receive>, Receive) -> Return> CoroutineFactory<Yield, Return, Receive, F> {
        pub fn new(handler: F) -> Self {
            Self(handler, PhantomData)
        }
        pub fn build<'a>(self) -> Coroutine<'a,Yield, Return, Receive> {
            let stack = ProtectedFixedSizeStack::default();
            let mut transfer=Transfer::new(unsafe { Context::new(&stack, run_co_context::<Yield, Return, Receive, F>) }, 0);
            transfer=unsafe {transfer.context.resume(ValueExchangeContainer::prepare_exchange(self.0).make_pointer())};
            let mut exchange_transfer=ExchangingTransfer::<ResumeType<Receive>,SuspenseType<Yield,Return>>::create_with_send(transfer);
            let co_channel=InvocationChannel::<Yield,Return,Receive>(exchange_transfer);
            let mut coroutine = Coroutine { state: InvocationState::Running(co_channel, stack) };
            coroutine
        }
    }

    pub struct Coroutine<'a,Yield, Return, Receive> {
        state: InvocationState<'a,Yield, Return, Receive>
    }

    #[derive(Debug)]
    pub enum ResumeResult<Yield, Return> {
        Yield(Yield),
        Return(Return),
    }

    pub enum CompleteVariant {
        Return,
        Unwind,
    }

    pub enum InvocationState<'a,Yield, Return, Receive> {
        Running(InvocationChannel<'a,Yield,Return,Receive>,ProtectedFixedSizeStack),
        Completed(CompleteVariant),
    }

    impl<'a,Yield, Return, Receive> Drop for Coroutine<'a,Yield, Return, Receive> {
        fn drop(&mut self) {
            match &mut self.state {
                InvocationState::Running(channel, _) => {
                    channel.unwind();
                }
                _ => {}
            }
        }
    }

    impl<'a,Yield, Return, Receive> Coroutine<'a,Yield, Return, Receive> {
        pub fn iter(&'a mut self) -> CoroutineIterator<Yield,Return,Receive> {
            CoroutineIterator(self)
        }
        pub fn resume(&mut self, send: Receive) -> ResumeResult<Yield, Return> {
            let rec = match &mut self.state {
                InvocationState::Running(channel, _) => channel.suspend(send),
                InvocationState::Completed(_) => panic!("tried to send to completed context")
            };
            self.receive(rec)
        }

        pub fn is_completed(&self) -> bool {
            match self.state {
                InvocationState::Completed(_) => true,
                _ => false
            }
        }

        fn receive(&mut self, rec: SuspenseType<Yield, Return>) -> ResumeResult<Yield, Return> {
            match rec {
                SuspenseType::Yield(y) => ResumeResult::Yield(y),
                SuspenseType::Complete(ct) => {
                    match ct {
                        CompleteType::Return(r) => {
                            self.state = InvocationState::Completed(CompleteVariant::Return);
                            ResumeResult::Return(r)
                        }
                        CompleteType::Unwind(u) => {
                            self.state = InvocationState::Completed(CompleteVariant::Unwind);
                            if let UnwindReason::Panic(p) = u {
                                resume_unwind(p)
                            } else {
                                panic!("coroutine context dropped outside of coroutine constructor")
                            }
                        }
                    }
                }
            }
        }
    }

    pub struct CoroutineChannel<'a,Yield,Return,Receive>(ExchangingTransfer<'a,SuspenseType<Yield,Return>,ResumeType<Receive>>,bool);
    pub struct InvocationChannel<'a,Yield,Return,Receive>(ExchangingTransfer<'a,ResumeType<Receive>,SuspenseType<Yield,Return>>);


    impl<'a,Yield,Return,Receive> CoroutineChannel<'a,Yield,Return,Receive> {
        pub fn suspend(&mut self,send:Yield) -> Receive {
            let received=self.0.yield_with(SuspenseType::Yield(send));
            self.receive(received)
        }

        fn receive(&mut self,r:ResumeType<Receive>) -> Receive {
            match r  {
                ResumeType::Yield(y) => y,
                ResumeType::Drop() =>{
                    self.1=true;
                    panic!()
                }
            }
        }
        fn complete_return(&mut self, ret:Return) -> ! {
            self.0.dispose_with(SuspenseType::Complete(CompleteType::Return(ret)))
        }

        fn complete_unwind(&mut self, reason:UnwindReason) -> ! {
            self.0.dispose_with(SuspenseType::Complete(CompleteType::Unwind(reason)))
        }
    }
    impl<'a,Yield,Return,Receive> InvocationChannel<'a,Yield,Return,Receive> {
        pub fn suspend(&mut self, send: Receive) -> SuspenseType<Yield,Return> {
            self.0.yield_with(ResumeType::Yield(send))
        }
        pub fn unwind(&mut self)  {
            match self.0.yield_with(ResumeType::Drop()) {
                SuspenseType::Complete(CompleteType::Unwind(_)) => (),
                _ => panic!("Invalid coroutine unwind result")
            }
        }
    }
    extern "C" fn run_co_context<Yield, Return, Receive, F: Fn(&mut CoroutineChannel<Yield, Return, Receive>, Receive) -> Return>(raw_transfer: Transfer) -> ! {
        let (mut exchange_transfer,routine_fn)=ExchangingTransfer::<SuspenseType<Yield,Return>,ResumeType<Receive>>::create_receiving::<F>(raw_transfer);
        let initial=exchange_transfer.suspend();
        let mut channel= CoroutineChannel(exchange_transfer,false);
        let result = catch_unwind(AssertUnwindSafe(|| {
            let initial=channel.receive(initial);
            routine_fn(&mut channel, initial)
        }));
        channel.0.dispose_with(SuspenseType::Complete(match result {
            Ok(ret) => CompleteType::Return(ret),
            Err(p) => CompleteType::Unwind(if channel.1 { UnwindReason::Drop } else { UnwindReason::Panic(p) })
        }))

    }

    pub struct CoroutineIterator<'a,Yield,Return,Receive>(&'a mut Coroutine<'a,Yield,Return,Receive>);
    impl<'a,Yield,Return> Iterator for CoroutineIterator<'a,Yield,Return,()> {
        type Item = Yield;

        fn next(&mut self) -> Option<Self::Item> {
            match self.0.resume(()) {
                ResumeResult::Return(r) => None,
                ResumeResult::Yield(y) => Some(y)
            }
        }
    }

    mod tests {
        use crate::coroutines::execution::{ ResumeType, SuspenseType};
        use context::stack::ProtectedFixedSizeStack;
        use context::{ContextFn, Transfer, Context};
        use crate::coroutines::transfer::{SuspendableValueTransfer, ValueMoveTransfer};
        use crate::utils::SelfUpdating;
        use std::marker::PhantomData;

        static mut STATIC_TEST_STACK: Option<ProtectedFixedSizeStack> = None;

        fn create_test_context(test_fn: ContextFn, start_data: usize) -> Transfer {
            unsafe {
                STATIC_TEST_STACK = Some(ProtectedFixedSizeStack::default())
            }
            unsafe { Transfer::new(Context::new(STATIC_TEST_STACK.as_ref().unwrap(), test_fn), start_data) }
        }

        extern "C" fn init_test(t: Transfer) -> ! {
            let mut value_trans = ValueMoveTransfer::new(t);
            value_trans.send_content::<SuspenseType<f64, i32>>(SuspenseType::Yield(0.4));
            panic!("")
        }

        #[test]
        fn context_channel_test() {
            let raw_t = create_test_context(init_test, 0);
            let mut channel = ContextChannel { send_transfer: SelfUpdating::of(SuspendableValueTransfer::init(raw_t)) };
            let t = channel.suspend_context::<ResumeType<u32>, SuspenseType<f64, i32>>(ResumeType::Yield(1));
            println!("{:?}", t)
        }
    }
}