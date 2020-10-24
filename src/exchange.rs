/**
*
*/
mod transfer {
    use std::mem::take;
    use context::Transfer;
    use std::intrinsics::transmute;
    use std::ops::{Deref, DerefMut};

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

    struct SelfUpdating<T>(Option<T>);

    impl<T> SelfUpdating<T> {
        fn of(initial: T) -> Self {
            Self(Some(initial))
        }

        fn update<F: Fn(T) -> T>(&mut self, op: F) {
            self.0 = Some(op(self.0.take().unwrap()))
        }
    }

    impl<T> Deref for SelfUpdating<T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            self.0.as_ref().unwrap()
        }
    }

    impl<T> DerefMut for SelfUpdating<T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            self.0.as_mut().unwrap()
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

    mod tests {
        use crate::exchange::transfer::ValueExchangeContainer;

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

    }
}