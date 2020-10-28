use std::marker::PhantomData;

use crate::coroutines::{Coroutine, CoroutineChannel, CoroutineFactory, ResumeResult, DynCoroutineFactory};

/// Trait implemented by all GeneratorFactorys
/// Designed to be implemented by Generator also copying IntoIterator semantics, but that turned out to be a problem
/// TODO maybe it is somehow possible to implement IntoGenerator for Generator
pub trait IntoGenerator {
    type GenYield:'static;
    type GenReturn:'static;
    type GenReceive;
    /// Returning a generator fulfilling implementors semantics with init callstack ready to be invoked/resumed
    fn build<'a>(self) -> Generator<'a, Self::GenYield,Self::GenReturn, Self::GenReceive> where Self::GenReceive:'a;
}

pub type GenFn<Yield, Return, Receive> = dyn FnOnce(&mut GeneratorChannel<Yield, Return, Receive>, Receive) -> Return;

/// Factory object wrapping generator closure for later instantiation (lazy init)
pub struct GeneratorFactory<Yield: 'static, Return: 'static, Receive>(Box<GenFn<Yield, Return, Receive>>, PhantomData<(Yield, Return, Receive)>);

pub trait PlainGenerator {
    type Yield;
    type Receive;
    /// Resumes the current execution of the generator by sending [val]
    /// Either returns Some<Y> if generators yields another value or None if generator completes before return
    /// After this method returned None once it may not be called another time or it will panic
    /// Use [has_completed] to determine execution state
    fn resume(&mut self, receive:Self::Receive)->Option<Self::Yield>;
    /// Quries whether Generator call has already completed or may be resumed
    fn has_completed(&self) -> bool;
}

pub trait ReturningGenerator:PlainGenerator {
    type Return;

    /// Converts this generator into it's result destructively
/// Caution: The Result determines whether generator closure has returned (Ok(Ret)) or generator callstack has been unwinded before return for some reason (Err())
/// If generator closure itself returns a Result this call Returns Result<Result<_,_>,()>
/// Panics if generator has not completed yet(thus no result exists)
     fn result(self) -> Result<Self::Return, ()>;
}
/// Decorator implementing generator semantics around a coroutine
/// Main entrance point for Generator usage
pub struct Generator<'a, Yield: 'static, Return: 'static, Receive: 'a>(GeneratorState<'a, Yield, Return, Receive>);

/// Tupe alias for Generator instances which do not receive meaningful input (and such can ignore it)
pub type PureGenerator<'a, Yield, Return> = Generator<'a, Yield, Return, ()>;

pub type MonoGenerator<'a, Yield, Receive> = Generator<'a, Yield, Yield, Receive>;

pub type PureMonoGenerator<'a, Yield> = MonoGenerator<'a, Yield, ()>;

/// Wrapper around CoroutineChannel passed to generator function/closure offering the possibility to yield values
pub struct GeneratorChannel<'a, 'b: 'a, Yield: 'static, Return: 'static, Receive: 'a>(&'a mut CoroutineChannel<'b, Yield, Return, Receive>);

/// Iterator over receiving generators containing a Closure as a source of input values
pub struct GeneratorIterator<'a, Yield: 'static, Return: 'static, Receive: 'a, RF: Fn() -> Receive>(Generator<'a, Yield, Return, Receive>, RF);

/// Holds the current execution state of the generator wrapping the invocation state of the Coroutine and buffering the extra return value
enum GeneratorState<'a, Yield: 'static, Return: 'static, Receive: 'a> {
    RUNNING(Coroutine<'a, Yield, Return, Receive>),
    COMPLETED(Return),
}

impl<Yield: 'static, Return: 'static, Receive> GeneratorFactory<Yield, Return, Receive> {
    fn new(handler: impl FnOnce(&mut GeneratorChannel<Yield, Return, Receive>, Receive) -> Return + 'static) -> Self {
        Self(Box::new(handler), PhantomData)
    }
}

impl<Yield: 'static, Return: 'static, Receive> IntoGenerator for GeneratorFactory<Yield, Return, Receive> {
    type GenYield = Yield;
    type GenReturn =Return;
    type GenReceive = Receive;

    fn build<'a>(self) -> Generator<'a, Yield, Return, Receive> where Receive:'a {
        let gen_fn = self.0;
        Generator(GeneratorState::RUNNING(CoroutineFactory::new(|con, i| {
            let mut generator_channel = GeneratorChannel(con);
            gen_fn(&mut generator_channel, i)
        }).build()))
    }
}

impl<'a, Y: 'static, Ret: 'static, Rec: 'a> ReturningGenerator for Generator<'a, Y, Ret, Rec> {
    type Return=Ret;
    fn result(self) -> Result<Self::Return, ()> {
        if self.has_completed() {
            match self.0 {
                GeneratorState::COMPLETED(r) => Ok(r),
                _ => Err(())
            }
        } else {
            panic!("generator hasn't completed yet")
        }
    }
}
impl<'a, Y: 'static, Ret: 'static, Rec: 'a> PlainGenerator for Generator<'a, Y, Ret, Rec> {
    type Yield = Y;
    type Receive = Rec;

    fn resume(&mut self, val: Self::Receive) -> Option<Self::Yield> {
        let next = match &mut self.0 {
            GeneratorState::RUNNING(co) => co.resume(val),
            GeneratorState::COMPLETED(_) => panic!("invalid generator state")
        };
        match next {
            ResumeResult::Return(r) => {
                self.0 = GeneratorState::COMPLETED(r);
                None
            }
            ResumeResult::Yield(v) => Some(v)
        }
    }

    fn has_completed(&self) -> bool {
        match &self.0 {
            GeneratorState::COMPLETED(_) => true,
            GeneratorState::RUNNING(co) => {
                co.is_completed()
            }
        }
    }
}

impl<'a, Y: 'static, Ret: 'static, Rec: 'a> Generator<'a, Y, Ret, Rec> {
    /// Factory function creating a new generator with input capabilities
    /// The factoring is eager: a Generator with allocated call stack and context will be returned
    pub fn new_receiving<F>(gen_fn: F) -> Generator<'a, Y, Ret, Rec>
        where F: FnOnce(&mut GeneratorChannel<Y, Ret, Rec>, Rec) -> Ret + 'static {
        Self::new_receiving_lazy(gen_fn).build()
    }

    /// Like [new_receiving] but lazy: a GeneratorFactory holding the generator closure is returned and context is allocated after .build() is called
    pub fn new_receiving_lazy<F>(gen_fn: F) -> impl IntoGenerator<GenYield=Y, GenReturn=Ret, GenReceive=Rec>
        where F: FnOnce(&mut GeneratorChannel<Y, Ret, Rec>, Rec) -> Ret + 'static {
        GeneratorFactory::new(gen_fn)
    }
}

impl<'a, Y: 'static, Ret: 'static> Generator<'a, Y, Ret, ()> {
    /// Create a generator which does not receive meaninful values and there may ignore it (closure does not receive initial argument as second parameter)
    /// Returns an initialized Generator with allocated callstack ready for iteration
    pub fn new<F>(gen_fn: F) -> Generator<'a, Y, Ret, ()>
        where F: FnOnce(&mut GeneratorChannel<Y, Ret, ()>) -> Ret + 'static{
        Self::new_lazy(gen_fn).build()
        //PureGeneratorFactory::new(gen_fn).build()
    }
    /// Same as [new] but returns a factory that need to be .build()
    pub fn new_lazy<F>(gen_fn: F) -> impl IntoGenerator<GenYield=Y, GenReturn=Ret, GenReceive=()>
        where F: FnOnce(&mut GeneratorChannel<Y, Ret, ()>) -> Ret + 'static {
        GeneratorFactory::new(|chan, _| gen_fn(chan))
    }
}


impl<'a, Y: 'static, Ret: 'static> Iterator for &mut Generator<'a, Y, Ret, ()> {
    type Item = Y;
    /// offers non destructive iteration
    fn next(&mut self) -> Option<Self::Item> {
        self.resume(())
    }
}


impl<'a, Y: 'a, Ret: 'a> IntoIterator for Generator<'a, Y, Ret, ()> {
    type Item = Y;
    type IntoIter = GeneratorIterator<'a, Y, Ret, (), fn()>;
    /// Iterator for non-receiving generators (do not need receive source closure)
    fn into_iter(self) -> Self::IntoIter {
        fn constant_identity() {}
        GeneratorIterator(self, constant_identity)
    }
}

impl<'a, 'b: 'a, Y: 'static, Ret: 'static, Rec: 'a> GeneratorChannel<'a, 'b, Y, Ret, Rec> {
    /// Send single [val] and yields execution
    pub fn yield_val(&mut self, val: Y) -> Rec {
        self.0.suspend(val)
    }
    /// yield all values from given [iter] Iterator one by one
    pub fn yield_all(&mut self, iter: impl IntoIterator<Item=Y>) {
        for i in iter {
            self.yield_val(i);
        }
    }
    /// Iterates given non-receiving Generator [gen] and returns the result afterwards
    pub fn yield_from<R>(&mut self, mut gen: Generator<Y, R, ()>) -> R {
        self.yield_all(&mut gen);
        gen.result().unwrap()
    }
}

impl<'a, Y, Ret, Rec, RF: Fn() -> Rec> Iterator for GeneratorIterator<'a, Y, Ret, Rec, RF> {
    type Item = Y;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.resume((self.1)())
    }
}