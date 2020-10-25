/**
*
*/
mod transfer {
    use std::mem::take;
    use context::Transfer;
    use std::intrinsics::transmute;
    use crate::utils::SelfUpdating;

    pub enum ValueExchangeContainer<V> {
        Value(V),
        Empty,
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

        fn make_pointer(&self) -> usize {
            if self.has_content() {
                unsafe { transmute::<*const Self, usize>(self as *const Self) }
            } else {
                panic!("pointer for empty exchange are forbidden")
            }
        }

        fn of_pointer<'a>(p: usize) -> &'a mut Self {
            unsafe {
                &mut *transmute::<usize, *mut Self>(p)
            }
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
        pub fn init(raw_transfer:Transfer) -> Self {
            Self(ValueMoveTransfer::new(raw_transfer))
        }

        pub fn receive<V>(self) -> (SuspendableValueTransfer,V) {
            let received_content=self.0.move_transfer_in();
            (SuspendableValueTransfer(self.0),received_content)
        }
    }

    impl SuspendableValueTransfer {
        pub fn init(raw_transfer:Transfer) -> Self {
            Self(ValueMoveTransfer::new(raw_transfer))
        }

        pub fn suspend<V>(mut self,content:V) -> ReceivableValueTransfer {
            self.0.send_content(content);
            ReceivableValueTransfer(self.0)
        }
    }

    mod tests {
        use crate::exchange::transfer::{ValueExchangeContainer, ValueMoveTransfer, ReceivableValueTransfer, SuspendableValueTransfer};
        use context::stack::ProtectedFixedSizeStack;
        use context::{ContextFn, Transfer, Context};

        #[test]
        fn exchange_container_prepare() {
            let container=ValueExchangeContainer::prepare_exchange(1);
            if let ValueExchangeContainer::Value(content)=container {
               assert_eq!(content,1)
            } else {
                panic!("value should exists")
            }
        }

        #[test]
        fn exchange_container_has_content_correct_result() {
            let container=ValueExchangeContainer::prepare_exchange(1);
            assert_eq!(container.has_content(),true);
            let container=ValueExchangeContainer::<usize>::Empty;
            assert_eq!(container.has_content(),false);
        }

        #[test]
        fn exchange_container_receive_content() {
            let mut container=ValueExchangeContainer::prepare_exchange(1);
            assert_eq!(container.receive_content(),1);
            assert_eq!(container.has_content(),false);
        }

        #[test]
        fn exchange_container_dup_by_pointer() {
            let exchange_container=ValueExchangeContainer::prepare_exchange(1);
            let dup_container = ValueExchangeContainer::<i32>::of_pointer(exchange_container.make_pointer());
            assert_eq!(dup_container.receive_content(),1)
        }

        static mut STATIC_TEST_STACK:Option<ProtectedFixedSizeStack> =None;

        fn create_test_context(test_fn:ContextFn, start_data:usize) -> Transfer {
            unsafe {
                STATIC_TEST_STACK = Some(ProtectedFixedSizeStack::default())
            }
           unsafe { Transfer::new(Context::new(STATIC_TEST_STACK.as_ref().unwrap(), test_fn), start_data) }
        }
        extern "C" fn init_test(_:Transfer) -> ! {
            panic!("")
        }
        #[test]
        fn value_transfer_init() {
            let test_transfer =create_test_context(init_test,2);
            let value_transfer=ValueMoveTransfer::new(test_transfer);
            assert_eq!(value_transfer.raw_transfer.data, 2)
        }

        #[test]
        fn value_transfer_receive() {
            let test_container=ValueExchangeContainer::prepare_exchange(2);
            let test_transfer =create_test_context(init_test,test_container.make_pointer());
            let value_transfer=ValueMoveTransfer::new(test_transfer);

            assert_eq!(value_transfer.move_transfer_in::<i32>(), 2)
        }

        #[test]
        fn value_transfer_send() {
            extern "C" fn send_test(t:Transfer) -> ! {
                assert_eq!(ValueExchangeContainer::<i32>::of_pointer(t.data).receive_content(),2);
                unsafe {t.context.resume(0);}
                panic!()
            }
            let test_transfer =create_test_context(send_test,0);
            let mut value_transfer=ValueMoveTransfer::new(test_transfer);
            value_transfer.send_content::<i32>(2);
        }

        #[test]
        fn receive_suspendable_transfer_cycle(){
            extern "C" fn send_test(t:Transfer) -> ! {
                let receive_transfer = ReceivableValueTransfer::init(t);
                let (suspend_transfer,rec)=receive_transfer.receive::<i32>();
                assert_eq!(rec,2);
                suspend_transfer.suspend(3);
                panic!()
            }
            let test_transfer =SuspendableValueTransfer::init(create_test_context(send_test,0));
            let receive_transfer=test_transfer.suspend(2);
            let (_,rec) = receive_transfer.receive::<i32>();
            assert_eq!(rec,3)

        }
    }
}

pub mod execution {
    use std::any::Any;
    use crate::utils::SelfUpdating;
    use crate::exchange::transfer::{SuspendableValueTransfer, ReceivableValueTransfer, ValueMoveTransfer, ValueExchangeContainer};
    use std::marker::PhantomData;
    use context::{Transfer, Context};
    use std::panic::{catch_unwind, AssertUnwindSafe, resume_unwind};
    use context::stack::ProtectedFixedSizeStack;

    type PanicData=Box<dyn Any+Send+'static>;
    #[derive(Debug)]
    enum UnwindReason {
        Panic(PanicData),
        Drop
    }
    #[derive(Debug)]
    enum CompleteType<Return> {
        Return(Return),
        Unwind(UnwindReason)
    }
    #[derive(Debug)]
    enum SuspenseType<Yield,Return> {
        Yield(Yield),
        Complete(CompleteType<Return>)
    }
    #[derive(Debug)]
    enum ResumeType<Receive> {
        Yield(Receive),
        Drop()
    }

    pub struct ContextChannel {
        send_transfer: SelfUpdating<SuspendableValueTransfer>
    }

    impl ContextChannel {
        fn new(transfer:SuspendableValueTransfer) -> Self {
            Self {send_transfer:SelfUpdating::of(transfer)}
        }

        fn suspend_context<SendMessage,ReceiveMessage>(&mut self,send:SendMessage) -> ReceiveMessage {
            self.send_transfer.returning_update(move |t| {
                let rec_transfer=t.suspend::<SendMessage>(send);
                rec_transfer.receive::<ReceiveMessage>()
            })
        }

        fn complete_context<SendMessage>(&mut self, send: SendMessage) -> ! {
            self.send_transfer.consume(move |t| {t.suspend(send);})
        }
    }

    pub struct CoroutineChannel<Yield,Return,Receive>(ContextChannel,bool,PhantomData<(Yield,Return,Receive)>);

    pub struct CoroutineInvoker<Yield,Return,Receive,F:Fn(&mut CoroutineChannel<Yield,Return,Receive>,Receive)->Return>(F,PhantomData<(Yield,Return,Receive)>);

    impl<Yield,Return,Receive,F:Fn(&mut CoroutineChannel<Yield,Return,Receive>,Receive)->Return> CoroutineInvoker<Yield,Return,Receive,F>{
        pub fn new(handler:F) -> Self {
            Self(handler,PhantomData)
        }
        pub fn invoke(self,initial:Receive) -> (Coroutine<Yield,Return,Receive>,ResumeResult<Yield,Return>) {
            let stack=ProtectedFixedSizeStack::default();
            let mut channel=
                ContextChannel::new(SuspendableValueTransfer::init(Transfer::new(unsafe {Context::new(&stack,run_co_context::<Yield,Return,Receive,F>)},0)));
            let rec=channel.suspend_context::<(F,Receive),SuspenseType<Yield,Return>>((self.0,initial));
            let mut coroutine = Coroutine { state: InvocationState::Running(stack,channel,PhantomData)};
            let rec = coroutine.receive(rec);
            (coroutine,rec)
        }
    }

    pub struct Coroutine<Yield,Return,Receive> {
        state:InvocationState<Yield,Return,Receive>
    }
    #[derive(Debug)]
    pub enum ResumeResult<Yield,Return> {
        Yield(Yield),
        Return(Return)
    }

    impl<Yield,Return,Receive> Coroutine<Yield,Return,Receive> {

        pub fn resume(&mut self,send:Receive) -> ResumeResult<Yield,Return> {
            let rec=match &mut self.state {
                InvocationState::Running(_, channel,_) =>
                    channel.suspend_context(ResumeType::Yield(send))
                ,
                InvocationState::Completed(_) => panic!("tried to send to completed context")
            };
            self.receive(rec)
        }

        pub fn is_completed(&self) -> bool {
            match self.state {
                InvocationState::Completed(_) =>true,
                _ => false
            }
        }

        fn receive(&mut self,rec:SuspenseType<Yield,Return>) -> ResumeResult<Yield,Return> {
            match rec {
                SuspenseType::Yield(y) => ResumeResult::Yield(y),
                SuspenseType::Complete(ct) => {
                    match ct {
                        CompleteType::Return(r) => {
                            self.state=InvocationState::Completed(CompleteVariant::Return);
                            ResumeResult::Return(r)
                        }
                        CompleteType::Unwind(u) => {
                            self.state=InvocationState::Completed(CompleteVariant::Unwind);
                            if let UnwindReason::Panic(p)=u {
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

    pub enum CompleteVariant {
        Return,Unwind
    }
    pub enum InvocationState<Yield,Return,Receive> {
        Running(ProtectedFixedSizeStack,ContextChannel,PhantomData<(Yield,Return,Receive)>),
        Completed(CompleteVariant)
    }

    impl<Yield,Return,Receive> CoroutineChannel<Yield,Return,Receive> {
        fn create(transfer:SuspendableValueTransfer) -> Self {
            Self(ContextChannel::new(transfer),false,PhantomData)
        }

        pub fn yield_with(&mut self,yield_val:Yield) -> Receive {
            let received=self.0.suspend_context::<SuspenseType<Yield,Return>,ResumeType<Receive>>(SuspenseType::Yield(yield_val));
            self.resume(received)
        }

        fn resume(&mut self,received_val:ResumeType<Receive>) -> Receive {
            match received_val {
                ResumeType::Yield(received_val) => received_val,
                ResumeType::Drop() => {
                    self.1=true;
                    panic!("Unwinding coroutine context due to coroutine drop")
                }
            }
        }

        fn return_with(&mut self, return_val:Return) -> ! {
            self.0.complete_context::<SuspenseType<Yield,Return>>(SuspenseType::Complete(CompleteType::Return(return_val)))
        }

        fn unwind(&mut self, unwind_reason:UnwindReason) -> ! {
            self.0.complete_context::<SuspenseType<Yield,Return>>(SuspenseType::Complete(CompleteType::Unwind(unwind_reason)))
        }
    }

    extern "C" fn run_co_context<Yield,Return,Receive,F:Fn(&mut CoroutineChannel<Yield,Return,Receive>,Receive)->Return>(raw_transfer:Transfer) -> !{
        let (suspend_transfer,(routine_fn,initial_receive))=ReceivableValueTransfer::init(raw_transfer).receive::<(F,Receive)>();
        let mut channel=CoroutineChannel::<Yield,Return,Receive>::create(suspend_transfer);
        // TODO check if AssertUnwindSafe may be replaced by bounds
        let result=catch_unwind(AssertUnwindSafe(|| {routine_fn(&mut channel,initial_receive)}));
        match result {
            Ok(ret) => channel.return_with(ret),
            Err(p) => channel.unwind(if channel.1 {UnwindReason::Drop} else {UnwindReason::Panic(p)})
        }
    }

    mod tests {
        use crate::exchange::execution::{ContextChannel, ResumeType, SuspenseType};
        use context::stack::ProtectedFixedSizeStack;
        use context::{ContextFn, Transfer, Context};
        use crate::exchange::transfer::{SuspendableValueTransfer, ValueMoveTransfer};
        use crate::utils::SelfUpdating;
        use std::marker::PhantomData;

        static mut STATIC_TEST_STACK:Option<ProtectedFixedSizeStack> =None;

        fn create_test_context(test_fn:ContextFn, start_data:usize) -> Transfer {
            unsafe {
                STATIC_TEST_STACK = Some(ProtectedFixedSizeStack::default())
            }
            unsafe { Transfer::new(Context::new(STATIC_TEST_STACK.as_ref().unwrap(), test_fn), start_data) }
        }
        extern "C" fn init_test(t:Transfer) -> ! {
            let mut value_trans=ValueMoveTransfer::new(t);
            value_trans.send_content::<SuspenseType<f64,i32>>(SuspenseType::Yield(0.4));
            panic!("")
        }
        #[test]
        fn context_channel_test() {
            let raw_t=create_test_context(init_test,0);
            let mut channel = ContextChannel {  send_transfer:SelfUpdating::of(SuspendableValueTransfer::init(raw_t)) };
            let t=channel.suspend_context::<ResumeType<u32>,SuspenseType<f64,i32>>(ResumeType::Yield(1));
           println!("{:?}",t)
        }
    }
}