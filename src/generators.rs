use crate::coroutines::{Coroutine, CoroutineChannel, ResumeResult};

/// General Closure signature that is used by full fletched Generator
pub type BoostedGenFn<Yield, Return, Receive> = dyn FnOnce(&mut BoostedGeneratorChannel<Yield, Return, Receive>, Receive) -> Return;

/// Base Generator trait representing the most fundamental generator functionality:
/// - types Yield and Receive telling which data generator emits and accepts
/// - methods resume which resumes execution
/// - method has_completed which queries state
pub trait Generator<'a>{
    type Yield:'static;
    type Receive: 'a;
    /// Determines whether this generators and its coroutine context have completed or are still resumeable
    fn has_completed(&self) -> bool;
    /// Resumes or starts execution of this generators callstack sending [send] to it
    /// Returns Option containing a value of type Yield in case generator yields a value and suspends or None of generator completes
    /// This method may not be called after it returned None once or behaviour is undefined(most likely this would cause a panic)
    /// [has_completed] will return true iif resume has returned None once
    fn resume(&mut self,send:Self::Receive) -> Option<Self::Yield>;
}

/// A ResultingGenerator is a [Generator] with the additional ability to return a value indepent of the yielded data
/// Can be useful to return summarize of error states etc.
pub trait ResultingGenerator<'a>:Generator<'a> {
    type Return:'static;
    /// Converts Generator into its resulting value whereby,
    /// Ok(r) means the generator has successfully generated a return value(which might be another Result as well)
    /// Err(()) means that generator stack has been unwinded before it's execution completed (most likely due to a panic)
    /// This methods panics if generator has not completed yet, i.e. [has_completed] returns false
    fn result(self) -> Result<Self::Return,()>;
}
/// Marker trait stating that Generator does not receive meaningful values. Thus it can be iterated over (with resume(()) without further information.
/// This was designed to genericly implement iterator (impl<G:IgnorantGenerator> Iterator for G like), but it turned out to be complicated. Such this trait is somewhat useless but kept for later ideas
/// TODO find better design approach
pub trait IgnorantGenerator<'a,Yield:'static>:Generator<'a,Yield=Yield,Receive=()>+Iterator<Item=Yield> {}

/// [GeneratorChannel] is the interface that connects the generating closure with the invocation context and provides a method to yield a value as well was utility methods handling iterator related stuff
pub trait GeneratorChannel<'a> {
    type Yield:'static;
    type Receive:'a;
    /// yields execution to waiting invocation context sending given [val]
    fn yield_val(&mut self,val:Self::Yield) -> Self::Receive;

    /// yields all values from given iterator
    fn yield_all(&mut self, iter: impl Iterator<Item=Self::Yield>) {
        for i in iter {
            self.yield_val(i);
        }
    }

    /// Flat yields a iterator of yield value iterators
    fn yield_all_flat<I:Iterator<Item=Self::Yield>>(&mut self, iters:impl Iterator<Item=I>) {
        for iter in iters {
            self.yield_all(iter);
        }
    }
    /// Iterates given non-receiving Generator [gen] and returns the result afterwards
    fn yield_from<R:'static>(&mut self, mut gen: impl IgnorantGenerator<'a,Self::Yield>+ResultingGenerator<'a,Yield=Self::Yield,Return=R, Receive=()>) -> R {
        self.yield_all(&mut gen);
        gen.result().unwrap()
    }
}

/// A simple Generator implementation only supporting non-receiving, ignorant generators by building a thin wrapper around Coroutines rearranging the user interface more or less
/// Not that flexible but straight forward to use
pub struct BoringGenerator<'a, Yield: 'static>(Coroutine<'a, Yield, (), ()>);

/// Channel implementation for [BoringGeneratorChannel]
/// TODO check whether generating closure may receive something like "impl GeneratorChannel" to be a) more generic and b) makes it possible to hide concrete structs
pub struct BoringGeneratorChannel<'a, 'b: 'a, Yield: 'static>(&'a mut CoroutineChannel<'b, Yield, (), ()>);

/// [Generator] implementation providing full-fledged resulting generators which might be ignorant but can also receive values
pub struct BoostedGenerator<'a, Yield: 'static, Return: 'static, Receive: 'a>(BoostedGeneratorState<'a, Yield, Return, Receive>);

/// Wrapper around CoroutineChannel passed to generator function/closure offering the possibility to yield values
pub struct BoostedGeneratorChannel<'a, 'b: 'a, Yield: 'static, Return: 'static, Receive: 'a>(&'a mut CoroutineChannel<'b, Yield, Return, Receive>);

/// Iterator over receiving generators containing a Closure as a source of input values
pub struct BoostedGeneratorIterator<'a, Yield: 'static, Return: 'static, Receive: 'a, RF: FnMut() -> Receive>(BoostedGenerator<'a, Yield, Return, Receive>, RF);

/// Holds the current execution state of the generator wrapping the invocation state of the Coroutine and buffering the extra return value
enum BoostedGeneratorState<'a, Yield: 'static, Return: 'static, Receive: 'a> {
    RUNNING(Coroutine<'a, Yield, Return, Receive>),
    COMPLETED(Return),
}

impl<'a, Yield: 'static> BoringGenerator<'a, Yield> {
    /// Creates a new BoringGenerator using [gen_fn] as generating function yielding its return value (there it must return data of type Yield)
    pub fn new_with_return<F>(gen_fn: F) -> Self where F: FnOnce(&mut BoringGeneratorChannel<Yield>) -> Yield + 'static {
        Self::new(|chan| {
            let ret_yield = gen_fn(chan);
            chan.yield_val(ret_yield);
        })
    }
    /// Creates a new BoringGenerator using [gen_fn] as generating function ignoring its return value
    pub fn new<F>(gen_fn: F) -> Self where F: FnOnce(&mut BoringGeneratorChannel<Yield>) + 'static {
        Self(Coroutine::new(|chan, _| {
            let mut gen_chan = BoringGeneratorChannel(chan);
            gen_fn(&mut gen_chan);
        }))
    }
}

impl<'a, Yield: 'static> Generator<'a> for BoringGenerator<'a, Yield> {
    type Yield = Yield;
    type Receive = ();

    fn has_completed(&self) -> bool {
        self.0.is_completed()
    }

    fn resume(&mut self, send: Self::Receive) -> Option<Self::Yield> {
        let resumed=if self.has_completed() {None} else {Some(self.0.resume(send))};
        match resumed {
            Some(ResumeResult::Yield(y)) => Some(y),
            _ => None
        }
    }
}

impl<'a, Yield:'static,G:Generator<'a,Yield=Yield,Receive=()>+Iterator<Item=Yield>> IgnorantGenerator<'a,Yield> for G {}

impl<'a, Yield: 'static> Iterator for BoringGenerator<'a, Yield> {
    type Item = Yield;

    fn next(&mut self) -> Option<Yield> {
        self.resume(())
    }
}

impl<'a, Y: 'static, Ret: 'static, Rec: 'a> BoostedGenerator<'a, Y, Ret, Rec> {
    /// Factory function creating a new generator with input capabilities
    pub fn new_receiving<F>(gen_fn: F) -> Self
        where F: FnOnce(&mut BoostedGeneratorChannel<Y, Ret, Rec>, Rec) -> Ret + 'static {
        Self(BoostedGeneratorState::RUNNING(Coroutine::new(|chan, i| {
            let mut gen_chan = BoostedGeneratorChannel(chan);
            gen_fn(&mut gen_chan,i)
        })))
    }
    /// Creates a iterator for a non-ignorant Generator using the passed [source] closure as source of receive values
    pub fn create_iter<RF:FnMut()->Rec>(self, source:RF) -> BoostedGeneratorIterator<'a,Y,Ret,Rec,RF> {
        BoostedGeneratorIterator(self,source)
    }
}

impl<'a, Y: 'static, Ret: 'static, Rec: 'a> ResultingGenerator<'a> for BoostedGenerator<'a, Y, Ret, Rec> {
    type Return = Ret;

    fn result(self) -> Result<Ret, ()> {
        if self.has_completed() {
            match self.0 {
                BoostedGeneratorState::COMPLETED(r) => Ok(r),
                _ => Err(())
            }
        } else {
            panic!("generator hasn't completed yet")
        }
    }
}
impl<'a, Y: 'static, Ret: 'static, Rec: 'a> Generator<'a> for BoostedGenerator<'a, Y, Ret, Rec> {
    type Yield = Y;
    type Receive = Rec;

    fn has_completed(&self) -> bool {
        match &self.0 {
            BoostedGeneratorState::COMPLETED(_) => true,
            BoostedGeneratorState::RUNNING(co) => {
                co.is_completed()
            }
        }
    }

    fn resume(&mut self, send: Self::Receive) -> Option<Self::Yield> {
        let next = match &mut self.0 {
            BoostedGeneratorState::RUNNING(co) => co.resume(send),
            BoostedGeneratorState::COMPLETED(_) => panic!("invalid generator state")
        };
        match next {
            ResumeResult::Return(r) => {
                self.0 = BoostedGeneratorState::COMPLETED(r);
                None
            }
            ResumeResult::Yield(v) => Some(v)
        }
    }
}

impl<'a, Y: 'static, Ret: 'static> BoostedGenerator<'a, Y, Ret, ()> {
    /// Create a generator which does not receive meaninful values and there may ignore it (closure does not receive initial argument as second parameter)
    /// Returns an initialized Generator with allocated callstack ready for iteration
    pub fn new<F>(gen_fn: F) -> Self
        where F: FnOnce(&mut BoostedGeneratorChannel<Y, Ret, ()>) -> Ret + 'static {
        Self::new_receiving(|chan, _| {
            gen_fn(chan)
        })
    }
}


impl<'a, Y: 'static, Ret: 'static> Iterator for BoostedGenerator<'a, Y, Ret, ()> {
    type Item = Y;
    /// offers non destructive iteration
    fn next(&mut self) -> Option<Self::Item> {
        self.resume(())
    }
}

impl<'a, 'b: 'a, Y: 'static> GeneratorChannel<'a> for BoringGeneratorChannel<'a, 'b, Y> {
    type Yield = Y;
    type Receive = ();

    /// Send single [val] and yields execution
    fn yield_val(&mut self, val: Y) {
        self.0.suspend(val)
    }
}

impl<'a, 'b: 'a, Y: 'static, Ret: 'static, Rec: 'a> GeneratorChannel<'a> for BoostedGeneratorChannel<'a, 'b, Y, Ret, Rec> {
    type Yield = Y;
    type Receive = Rec;

    /// Send single [val] and yields execution
    fn yield_val(&mut self, val: Y) -> Rec {
        self.0.suspend(val)
    }
}

impl<'a, Y, Ret, Rec, RF: Fn() -> Rec> Iterator for BoostedGeneratorIterator<'a, Y, Ret, Rec, RF> {
    type Item = Y;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.resume((self.1)())
    }
}