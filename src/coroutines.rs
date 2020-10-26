use std::any::Any;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use context::{Context, Transfer};
use context::stack::ProtectedFixedSizeStack;

use crate::transfer::{ExchangingTransfer, ValueExchangeContainer};

/// Type alias for the data a panic is carrying
type PanicData = Box<dyn Any + Send + 'static>;

/// Encodes the reason the execution flow of a coroutine context has been resumed(or started) from an invoking context
/// Normally resume happens because the invoking context has passed a value (e.g. by channel.resume() in order to invoke or resume coroutines normal execution
/// Otherwise the invoking context is about to drop the controlling coroutine struct which requires the coroutine context to unwind its callstack
#[derive(Debug)]
pub enum ResumeType<Receive> {
    Yield(Receive),
    Drop(),
}

/// The reason a coroutine execution got suspended encoded to be communicated between invocation contexts.
/// The coroutine either got suspended in the middle of execution to yield a value(e.g.) by channel.suspend() call and is ready to resume execution after Yield variant has been send
/// Or the coroutine has completed execution - either by returning a value or by unwinding callstack for some reason - and may not be resumed after Complete variant has been send
#[derive(Debug)]
pub enum SuspenseType<Yield, Return> {
    Yield(Yield),
    Complete(CompleteType<Return>),
}

/// Encodes the variant of coroutines execution completion.
/// Either routines function has returned - in which case Return carries the returned value -
/// or the coroutine callstack has been unwinded - then Unwind carries the reason for unwinding
#[derive(Debug)]
pub enum CompleteType<Return> {
    Return(Return),
    Unwind(UnwindReason),
}

/// Encodes the reason a coroutine context has unwinded its callstack for
/// Either as panic occured while executing routine:
/// In this case panic data is transferred between context borders by Panic variant and is expected to be "rethrown" in invoking context
///
/// Otherwise invoking context instructed coroutine context to unwind its stack and Drop variant acknowledges successfull unwind
#[derive(Debug)]
pub enum UnwindReason {
    Panic(PanicData),
    Drop,
}

/// CoroutineFactory holds the closure and offer a method needed to construct an invocable coroutine
/// Creating a factory can enable the user to separate coroutine definition from invocation and postpones callstack/context creation as well as choosing invocation value until actual execution needs to happen
/// Also it is quite a helpful method to get rid of closure template parameter(which otherwise gets quite annoying) before generator struct is formed
pub struct CoroutineFactory<Yield, Return, Receive, F: FnOnce(&mut CoroutineChannel<Yield, Return, Receive>, Receive) -> Return>(F, PhantomData<(Yield, Return, Receive)>);

/// Represents the actual execution of a coroutine on invocation context side
/// It encapsulates a state enum being either in Running state holding context/stack or in Completed state holding completion type
/// It's methods offer the main public interface for invocation interaction
pub struct Coroutine<'a, Yield, Return, Receive>(InvocationState<'a, Yield, Return, Receive>);

#[derive(Debug)]
pub enum ResumeResult<Yield, Return> {
    Yield(Yield),
    Return(Return),
}

pub enum CompleteVariant {
    Return,
    Unwind,
}

pub enum InvocationState<'a, Yield, Return, Receive> {
    Running(InvocationChannel<'a, Yield, Return, Receive>, ProtectedFixedSizeStack),
    Completed(CompleteVariant),
}

pub struct CoroutineChannel<'a, Yield, Return, Receive>(ExchangingTransfer<'a, SuspenseType<Yield, Return>, ResumeType<Receive>>, bool);

pub struct InvocationChannel<'a, Yield, Return, Receive>(ExchangingTransfer<'a, ResumeType<Receive>, SuspenseType<Yield, Return>>);

impl<Yield, Return, Receive, F> CoroutineFactory<Yield, Return, Receive, F> where
    F: FnOnce(&mut CoroutineChannel<Yield, Return, Receive>, Receive) -> Return {
    pub fn new(handler: F) -> Self {
        Self(handler, PhantomData)
    }
    pub fn build<'a>(self) -> Coroutine<'a, Yield, Return, Receive> {
        let stack = ProtectedFixedSizeStack::default();
        let transfer = unsafe {
            Transfer::new(Context::new(&stack, run_co_context::<Yield, Return, Receive, F>), 0).context.resume(ValueExchangeContainer::prepare_exchange(self.0).make_pointer())
        };
        Coroutine(InvocationState::Running(InvocationChannel::<Yield, Return, Receive>(ExchangingTransfer::<ResumeType<Receive>, SuspenseType<Yield, Return>>::create_with_send(transfer)), stack))
    }
}

impl<'a, Yield, Return, Receive> Drop for Coroutine<'a, Yield, Return, Receive> {
    fn drop(&mut self) {
        match &mut self.0 {
            InvocationState::Running(channel, _) => {
                channel.unwind();
            }
            _ => {}
        }
    }
}

impl<'a, Yield, Return, Receive> Coroutine<'a, Yield, Return, Receive> {
    pub fn resume(&mut self, send: Receive) -> ResumeResult<Yield, Return> {
        let rec = match &mut self.0 {
            InvocationState::Running(channel, _) => channel.suspend(send),
            InvocationState::Completed(_) => panic!("tried to send to completed context")
        };

        self.receive(rec)
    }

    pub fn is_completed(&self) -> bool {
        match self.0 {
            InvocationState::Completed(_) => true,
            _ => false
        }
    }

    fn receive(&mut self, rec: SuspenseType<Yield, Return>) -> ResumeResult<Yield, Return> {
        match rec {
            SuspenseType::Yield(y) => ResumeResult::Yield(y),
            SuspenseType::Complete(complete_type) => {
                match complete_type {
                    CompleteType::Return(r) => {
                        self.0 = InvocationState::Completed(CompleteVariant::Return);
                        ResumeResult::Return(r)
                    }
                    CompleteType::Unwind(u) => {
                        self.0 = InvocationState::Completed(CompleteVariant::Unwind);
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

impl<'a, Yield, Return, Receive> CoroutineChannel<'a, Yield, Return, Receive> {
    pub fn suspend(&mut self, send: Yield) -> Receive {
        let received = self.0.yield_with(SuspenseType::Yield(send));
        self.receive(received)
    }

    fn receive(&mut self, r: ResumeType<Receive>) -> Receive {
        match r {
            ResumeType::Yield(y) => y,
            ResumeType::Drop() => {
                self.1 = true;
                panic!("unwinding coroutine stack for drop")
            }
        }
    }
}

impl<'a, Yield, Return, Receive> InvocationChannel<'a, Yield, Return, Receive> {
    pub fn suspend(&mut self, send: Receive) -> SuspenseType<Yield, Return> {
        let t = self.0.yield_with(ResumeType::Yield(send));
        t
    }
    pub fn unwind(&mut self) {
        match self.0.yield_with(ResumeType::Drop()) {
            SuspenseType::Complete(CompleteType::Unwind(_)) => (),
            _ => panic!("Invalid coroutine unwind result")
        }
    }
}

extern "C" fn run_co_context<Yield, Return, Receive, F: FnOnce(&mut CoroutineChannel<Yield, Return, Receive>, Receive) -> Return>(raw_transfer: Transfer) -> ! {
    let (mut exchange_transfer, routine_fn) = ExchangingTransfer::<SuspenseType<Yield, Return>, ResumeType<Receive>>::create_receiving::<F>(raw_transfer);
    let initial = exchange_transfer.suspend();
    let mut channel = CoroutineChannel(exchange_transfer, false);
    let result = catch_unwind(AssertUnwindSafe(|| {
        let initial = channel.receive(initial);
        let t = routine_fn(&mut channel, initial);
        t
    }));
    channel.0.dispose_with(SuspenseType::Complete(match result {
        Ok(ret) => CompleteType::Return(ret),
        Err(p) => CompleteType::Unwind(if channel.1 { UnwindReason::Drop } else { UnwindReason::Panic(p) })
    }))
}

mod tests {
    use context::{Context, ContextFn, Transfer};
    use context::stack::ProtectedFixedSizeStack;

    static mut STATIC_TEST_STACK: Option<ProtectedFixedSizeStack> = None;

    fn create_test_context(test_fn: ContextFn, start_data: usize) -> Transfer {
        unsafe {
            STATIC_TEST_STACK = Some(ProtectedFixedSizeStack::default())
        }
        unsafe { Transfer::new(Context::new(STATIC_TEST_STACK.as_ref().unwrap(), test_fn), start_data) }
    }
}
