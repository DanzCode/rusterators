use crate::coroutines::execution::{Coroutine, CoroutineFactory, ResumeResult, CoroutineChannel};
use std::mem::{swap, replace};
use crate::coroutines::transfer::ValueExchangeContainer;
use std::marker::PhantomData;

pub struct ReceivingGeneratorFactory<Yield, Return, Receive, F>(F, PhantomData<(Yield, Return, Receive)>) where F: Fn(&mut GeneratorChannel<Yield, Return, Receive>, Receive) -> Return;

pub struct PureGeneratorFactory<Yield, Return, F>(F, PhantomData<(Yield, Return)>) where F: Fn(&mut GeneratorChannel<Yield, Return, ()>) -> Return;

pub struct Generator<'a, Yield, Return, Receive>(GeneratorState<'a, Yield, Return, Receive>);

pub struct GeneratorChannel<'a, 'b: 'a, Yield, Return, Receive>(&'a mut CoroutineChannel<'b, Yield, Return, Receive>);

pub struct GeneratorIterator<'a, Yield, Return, Receive, RF: Fn() -> Receive>(Generator<'a, Yield, Return, Receive>, RF);

pub struct OuterIterator<'a, Yield>(&'a mut Generator<'a, Yield, (), ()>);

enum GeneratorState<'a, Yield, Return, Receive> {
    RUNNING(Coroutine<'a, Yield, Return, Receive>),
    COMPLETED(Return),
}

pub trait IntoGenerator<Yield, Return, Receive> {
    fn build<'a>(self) -> Generator<'a, Yield, Return, Receive>;
}

impl<Yield, Return, F> PureGeneratorFactory<Yield, Return, F> where F: Fn(&mut GeneratorChannel<Yield, Return, ()>) -> Return {
    fn new(handler: F) -> Self {
        Self(handler, PhantomData)
    }
}

impl<Yield, Return, F> IntoGenerator<Yield, Return, ()> for PureGeneratorFactory<Yield, Return, F> where F: Fn(&mut GeneratorChannel<Yield, Return, ()>) -> Return {
    fn build<'a>(self) -> Generator<'a, Yield, Return, ()> {
        let gen_fn = self.0;
        ReceivingGeneratorFactory::new(|con: &mut GeneratorChannel<Yield, Return, ()>, _: ()| gen_fn(con)).build()
    }
}

impl<Yield, Return, Receive, F> IntoGenerator<Yield, Return, Receive> for ReceivingGeneratorFactory<Yield, Return, Receive, F> where F: Fn(&mut GeneratorChannel<Yield, Return, Receive>, Receive) -> Return {
    fn build<'a>(self) -> Generator<'a, Yield, Return, Receive> {
        let gen_fn = self.0;
        Generator(GeneratorState::RUNNING(CoroutineFactory::new(|con, i| {
            let mut generator_channel = GeneratorChannel(con);
            gen_fn(&mut generator_channel, i)
        }).build()))
    }
}

impl<Yield, Return, Receive, F> ReceivingGeneratorFactory<Yield, Return, Receive, F> where F: Fn(&mut GeneratorChannel<Yield, Return, Receive>, Receive) -> Return {
    fn new(handler: F) -> Self {
        Self(handler, PhantomData)
    }
}

impl<'a, 'b: 'a, Y, Ret, Rec> GeneratorChannel<'a, 'b, Y, Ret, Rec> {
    pub fn yield_val(&mut self, val: Y) -> Rec {
        self.0.suspend(val)
    }

    pub fn yield_all(&mut self, iter:impl IntoIterator<Item=Y>) {
        for i in iter {
            self.yield_val(i);
        }
    }

    pub fn yield_from<R>(&mut self, mut gen:Generator<Y,R,()>) -> Result<R,()>{
        self.yield_all(&mut gen);
        gen.result()
    }
}

impl<'a, Y, Ret, Rec, RF: Fn() -> Rec> Iterator for GeneratorIterator<'a, Y, Ret, Rec, RF> {
    type Item = Y;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.resume((self.1)())
    }
}

impl<'a, Y, Ret> Iterator for &mut Generator<'a, Y, Ret, ()> {
    type Item = Y;

    fn next(&mut self) -> Option<Self::Item> {
        self.resume(())
    }
}


impl<'a, Y: 'a, Ret: 'a> IntoIterator for Generator<'a,Y,Ret,()> {
    type Item = Y;
    type IntoIter = GeneratorIterator<'a, Y, Ret, (), fn()>;

    fn into_iter(mut self) -> Self::IntoIter {
        fn constant_identity() {}
        GeneratorIterator(self, constant_identity)
    }
}


impl<'a, Y, Ret> Generator<'a, Y, Ret, ()> {
    pub fn new<F: Fn(&mut GeneratorChannel<Y, Ret, ()>) -> Ret>(gen_fn:F) -> Generator<'a,Y,Ret,()>{
        PureGeneratorFactory::new(gen_fn).build()
    }
    pub fn new_lazy< F: Fn(&mut GeneratorChannel<Y, Ret, ()>) -> Ret>(gen_fn:F) -> PureGeneratorFactory<Y,Ret,F>{
        PureGeneratorFactory::new(gen_fn)
    }
}
impl<'a, Y, Ret, Rec> Generator<'a, Y, Ret, Rec> {
    pub fn new_receiving<F: Fn(&mut GeneratorChannel<Y, Ret, Rec>, Rec) -> Ret>(gen_fn:F) -> Generator<'a,Y,Ret,Rec>{
        ReceivingGeneratorFactory::new(gen_fn).build()
    }
    pub fn new_receiving_lazy< F: Fn(&mut GeneratorChannel<Y, Ret, Rec>, Rec) -> Ret>(gen_fn:F) -> ReceivingGeneratorFactory<Y,Ret,Rec,F>{
        ReceivingGeneratorFactory::new(gen_fn)
    }

    pub fn has_completed(&self) -> bool {
        match &self.0 {
            GeneratorState::COMPLETED(_) => true,
            GeneratorState::RUNNING(co) => {
                co.is_completed()
            }
        }
    }

    pub fn result(self) -> Result<Ret, ()> {
        if self.has_completed() {
            match self.0 {
                GeneratorState::COMPLETED(r) => Ok(r),
                _ => Err(())
            }
        } else {
            panic!("generator hasn't completed yet")
        }
    }

    pub fn resume(&mut self, val: Rec) -> Option<Y> {
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
}