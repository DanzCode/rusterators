use std::marker::PhantomData;

use crate::coroutines::{Coroutine, CoroutineChannel, ResumeResult};
use std::convert::TryInto;

/// General Closure signature that is used by full fletched Generator
pub type GenFn<Yield, Return, Receive> = dyn FnOnce(&mut BoostedGeneratorChannel<Yield, Return, Receive>, Receive) -> Return;

pub trait Generator<'a>{
    type Yield:'static;
    type Receive: 'a;

    fn has_completed(&self) -> bool;

    fn resume(&mut self,send:Self::Receive) -> Option<Self::Yield>;
}

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

pub trait ResultingGenerator<'a>:Generator<'a> {
    type Return:'static;

    fn result(self) -> Result<Self::Return,()>;
}

pub trait IgnorantGenerator<'a,Yield:'static>:Generator<'a,Yield=Yield,Receive=()>+Iterator<Item=Yield> {
}

pub struct MonoGenerator<'a, Yield: 'static>(Coroutine<'a, Yield, (), ()>);

pub struct MonoGeneratorChannel<'a, 'b: 'a, Yield: 'static>(&'a mut CoroutineChannel<'b, Yield, (), ()>);

/// Decorator implementing generator semantics around a coroutine
/// Main entrance point for Generator usage
pub struct BoostedGenerator<'a, Yield: 'static, Return: 'static, Receive: 'a>(BoostedGeneratorState<'a, Yield, Return, Receive>);

/// Wrapper around CoroutineChannel passed to generator function/closure offering the possibility to yield values
pub struct BoostedGeneratorChannel<'a, 'b: 'a, Yield: 'static, Return: 'static, Receive: 'a>(&'a mut CoroutineChannel<'b, Yield, Return, Receive>);

/// Iterator over receiving generators containing a Closure as a source of input values
pub struct BoostedGeneratorIterator<'a, Yield: 'static, Return: 'static, Receive: 'a, RF: Fn() -> Receive>(BoostedGenerator<'a, Yield, Return, Receive>, RF);

/// Holds the current execution state of the generator wrapping the invocation state of the Coroutine and buffering the extra return value
enum BoostedGeneratorState<'a, Yield: 'static, Return: 'static, Receive: 'a> {
    RUNNING(Coroutine<'a, Yield, Return, Receive>),
    COMPLETED(Return),
}

impl<'a, Yield: 'static> MonoGenerator<'a, Yield> {
    pub fn new_with_return<F>(gen_fn: F) -> Self where F: FnOnce(&mut MonoGeneratorChannel<Yield>) -> Yield + 'static {
        Self::new(|mut chan| {
            let ret_yield = gen_fn(chan);
            chan.yield_val(ret_yield);
        })
    }

    pub fn new<F>(gen_fn: F) -> Self where F: FnOnce(&mut MonoGeneratorChannel<Yield>) + 'static {
        Self(Coroutine::new(|chan, _| {
            let mut gen_chan = MonoGeneratorChannel(chan);
            gen_fn(&mut gen_chan);
        }))
    }
}

impl<'a, Yield: 'static> Generator<'a> for MonoGenerator<'a, Yield> {
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

impl<'a, Yield: 'static> Iterator for MonoGenerator<'a, Yield> {
    type Item = Yield;

    fn next(&mut self) -> Option<Yield> {
        self.resume(())
    }
}

impl<'a, Y: 'static, Ret: 'static, Rec: 'a> BoostedGenerator<'a, Y, Ret, Rec> {
    /// Factory function creating a new generator with input capabilities
    /// The factoring is eager: a Generator with allocated call stack and context will be returned
    pub fn new_receiving<F>(gen_fn: F) -> Self
        where F: FnOnce(&mut BoostedGeneratorChannel<Y, Ret, Rec>, Rec) -> Ret + 'static {
        Self(BoostedGeneratorState::RUNNING(Coroutine::new(|chan, i| {
            let mut gen_chan = BoostedGeneratorChannel(chan);
            gen_fn(&mut gen_chan,i)
        })))
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

impl<'a, 'b: 'a, Y: 'static> GeneratorChannel<'a> for MonoGeneratorChannel<'a, 'b, Y> {
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