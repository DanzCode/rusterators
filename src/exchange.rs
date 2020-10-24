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

    struct ReceivableValueTransfer(ValueMoveTransfer);
    struct SuspendableValueTransfer(ValueMoveTransfer);

    impl ReceivableValueTransfer {
        fn init(raw_transfer:Transfer) -> Self {
            Self(ValueMoveTransfer::new(raw_transfer))
        }

        fn receive<V>(self) -> (SuspendableValueTransfer,V) {
            let received_content=self.0.move_transfer_in();
            (SuspendableValueTransfer(self.0),received_content)
        }
    }

    impl SuspendableValueTransfer {
        fn init(raw_transfer:Transfer) -> Self {
            Self(ValueMoveTransfer::new(raw_transfer))
        }

        fn suspend<V>(mut self,content:V) -> ReceivableValueTransfer {
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