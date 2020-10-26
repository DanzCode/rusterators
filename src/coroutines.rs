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

/// Represents the return of a coroutine invocation/resume
/// While ResumeType/SuspenseType encode controlflow informations between the contexts, this type encode the user-side information
/// i.e. whether the routine has yielded a value ready to resume or returned a value and therefore completed. Panics however will be rethrown at a lower level and won't return at all
/// It will be returned by methods invoking the coroutine from the invocation context side (channel.resume()).
#[derive(Debug)]
pub enum ResumeResult<Yield, Return> {
    Yield(Yield),
    Return(Return),
}

/// Holds information of the way a coroutine completed execution.
/// In contrast to CompleteType, which is used to transfer controlflow information between contexts, this type encodes information for the calling user and therefore does not carry additional data.
/// This is because if variant is Return, channel.resume has already returned ResumeType::Return containing the return value
/// In case of a unwind, the Coroutine struct either dropped (in which case the variant can never be queried) or invocation paniced.
/// In later case panic has been rethrown on invocation side and therefore - if variant is queried - has been catched.
pub enum CompleteVariant {
    Return,
    Unwind,
}

/// Represents the current state of a coroutine execution.
/// If coroutine callstack and context have already been created(even if actual routine closure has not been invoked initially),
/// Running variant holds associated context structures and communication channel(meaning that all context including stack will be dropped as soon as state changes and such resources are freed as soon as possible)
/// Completed variant is used in case coroutine context has been dropped (either due to return or unwind) and controlling struct on invocation side still exists
enum InvocationState<'a, Yield, Return, Receive> {
    Running(InvocationChannel<'a, Yield, Return, Receive>, ProtectedFixedSizeStack),
    Completed(CompleteVariant),
}

/// Offers communication interface between contexts on coroutine context sides
/// Also holds information whether a caught panic is "real" or caused intentionally for controlled stack unwinding(second field is true in later case)
/// TODO: maybe this can be done in a better way
///
/// Provides possibility to suspend current execution by yielding a given value to invocation context and receiving a value sended by invocation context on return
pub struct CoroutineChannel<'a, Yield, Return, Receive>(ExchangingTransfer<'a, SuspenseType<Yield, Return>, ResumeType<Receive>>, bool);

/// Offers communication interface between contexts on invocation context side
/// Provides possibility to resume coroutine execution which kinds of equals CoroutineChannels suspend capability
/// However this is decorated by coroutine and not accessible outside
struct InvocationChannel<'a, Yield, Return, Receive>(ExchangingTransfer<'a, ResumeType<Receive>, SuspenseType<Yield, Return>>);

impl<Yield, Return, Receive, F> CoroutineFactory<Yield, Return, Receive, F> where
    F: FnOnce(&mut CoroutineChannel<Yield, Return, Receive>, Receive) -> Return {
    /// Constructs new factory out of coroutine closure
    pub fn new(handler: F) -> Self {
        Self(handler, PhantomData)
    }
    /// Build actual Coroutine by allocation stack, initing context information and transferring closure
    /// Inits Coroutine structure in initial Running state ready to be invoked
    pub fn build<'a>(self) -> Coroutine<'a, Yield, Return, Receive> {
        let stack = ProtectedFixedSizeStack::default();
        let transfer = unsafe {
            Transfer::new(Context::new(&stack, run_co_context::<Yield, Return, Receive, F>), 0).context.resume(ValueExchangeContainer::prepare_exchange(self.0).make_pointer())
        };
        Coroutine(InvocationState::Running(InvocationChannel::<Yield, Return, Receive>(ExchangingTransfer::<ResumeType<Receive>, SuspenseType<Yield, Return>>::create_with_send(transfer)), stack))
    }
}

impl<'a, Yield, Return, Receive> Drop for Coroutine<'a, Yield, Return, Receive> {
    /// Causes coroutine context to unwind in case it is still running
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
    /// Sends a given value to the coroutine context and yields execution control to it
    /// Returns either a Yield or a Return ResumeResult after coroutine execution has been suspended
    /// Panics in case coroutine execution did panic or in case coroutine execution already has completed it
    pub fn resume(&mut self, send: Receive) -> ResumeResult<Yield, Return> {
        let rec = match &mut self.0 {
            InvocationState::Running(channel, _) => channel.suspend(send),
            InvocationState::Completed(_) => panic!("tried to send to completed context")
        };

        self.receive(rec)
    }
    /// queries whether coroutine has completed execution
    pub fn is_completed(&self) -> bool {
        match self.0 {
            InvocationState::Completed(_) => true,
            _ => false
        }
    }
    /// Internally handles value passed by coroutine execution
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
    /// Suspends execution control to invocation context yielding the given value and waits for resume
    /// On resume it returns the value yielded by other contexts resume call
    pub fn suspend(&mut self, send: Yield) -> Receive {
        let received = self.0.yield_with(SuspenseType::Yield(send));
        self.receive(received)
    }

    /// Internally handles transferred message
    /// In case of a Yield just returns encapsulated value
    /// In case of a Drop a panic is thrown after marking panic as "controlled stack unwind"
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
    /// resumes execution of coroutine context yielding given value and waits for next suspend returning the encoded control flow type (Yield/Complete see [SuspenseType] and parameters)
    fn suspend(&mut self, send: Receive) -> SuspenseType<Yield, Return> {
        let t = self.0.yield_with(ResumeType::Yield(send));
        t
    }
    /// Causes coroutine execution context to unwind and checks whether consistent result is archieved
    fn unwind(&mut self) {
        match self.0.yield_with(ResumeType::Drop()) {
            SuspenseType::Complete(CompleteType::Unwind(_)) => (),
            _ => panic!("Invalid coroutine unwind result")
        }
    }
}

/// "Bootstrap" function for coroutine context
/// This wraps baremetal Boost:context execution by receiving closure struct, initing communication channel and wrapping closure execution in order to have a clean stack unwind in any case
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
/// a lot of really good tests
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
