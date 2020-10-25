use crate::coroutines::execution::{CoroutineChannel, Coroutine, CoroutineInvoker, ResumeResult};
use std::mem::{swap, replace};
use crate::coroutines::transfer::ValueExchangeContainer;

pub struct ReceivingGenerator<Yield,Return,Receive,F:Fn(&mut GeneratorChannel<Yield,Return,Receive>,Receive)->Return>(GeneratorState<Yield,Return,Receive,F>);

pub enum GeneratorState<Yield,Return,Receive,F:Fn(&mut GeneratorChannel<Yield,Return,Receive>,Receive)->Return> {
    Init(ValueExchangeContainer<F>),
    Run(Coroutine<Yield,Return,Receive>),
    Returned(Result<Return,()>)
}

pub struct GeneratorChannel<'a,Yield,Return,Receive>(&'a mut CoroutineChannel<Yield,Return,Receive>);
impl<'a,Yield,Return,Receive> GeneratorChannel<'a,Yield,Return,Receive> {
    pub fn yield_val(&mut self,val:Yield) -> Receive {
        self.0.yield_with(val)
    }
}

pub struct GeneratorIterator<'a,Yield,Return,Receive,RF:Fn()->Receive,F:Fn(&mut GeneratorChannel<Yield,Return,Receive>,Receive)->Return>(&'a mut ReceivingGenerator<Yield,Return,Receive,F>,RF);
impl<'a,Yield,Return,Receive,RF:Fn()->Receive,F:Fn(&mut GeneratorChannel<Yield,Return,Receive>,Receive)->Return> Iterator for GeneratorIterator<'a,Yield,Return,Receive,RF,F> {
    type Item = Yield;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.resume(self.1())
    }
}

struct Generator<Yield,Return>(ReceivingGenerator<Yield,Return,(),fn(&mut GeneratorChannel<Yield,Return,()>,())->Return>);
impl<Yield,Return> Generator<Yield,Return> {
     fn new<F:Fn(&mut GeneratorChannel<Yield,Return,()>)->Return>(gen_fn:F) -> Self {
        Self(ReceivingGenerator::new(|con:&mut GeneratorChannel<Yield,Return,()>,_| gen_fn(con)))
    }


    pub fn iter(&mut self) -> GeneratorIterator<Yield,Return,(),fn(),fn(&mut GeneratorChannel<Yield,Return,()>,())->Return> {
        let t=self.0.build_iterator(|| ());
    }
}
impl<Yield,Return,Receive,F:Fn(&mut GeneratorChannel<Yield,Return,Receive>,Receive)->Return> ReceivingGenerator<Yield,Return,Receive,F> {
    pub fn new(gen_fn:F) -> Self {
        ReceivingGenerator(GeneratorState::Init(gen_fn.into()))
    }

    pub fn build_iterator<RF: Fn() -> Receive>(&mut self, receive_source:RF) -> GeneratorIterator<Yield, Return, Receive, RF, F> {
        GeneratorIterator(self,receive_source)
    }

    pub fn resume(&mut self, send: Receive)->Option<Yield> {
         let rec=match &mut self.0 {
             GeneratorState::Init(h) => {
                 let handler=h.receive_content();
                 let invoke=CoroutineInvoker::new(|con,i| {
                     let mut channel=GeneratorChannel(con);
                     handler(&mut channel,i)
                 }).invoke(send);
                 replace(&mut self.0,GeneratorState::<Yield,Return,Receive,F>::Run(invoke.0));
                 invoke.1
             },
             GeneratorState::Run(c) => {
                 c.resume(send)
             },
             _ => panic!("Invalid generator state")
         };
        match rec {
            ResumeResult::Yield(y) => Some(y),
            ResumeResult::Return(r) => {
                replace(&mut self.0, GeneratorState::Returned(Ok(r)));
                None
            }
        }
    }

    pub fn has_completed(self) ->bool {
        match self.0 {
            GeneratorState::Returned(_) => true,
            _ => false
        }
    }

    pub fn generator_result(self) -> Option<Result<Return,()>> {
        match self.0 {
            GeneratorState::Returned(r) => Some(r),
            _ => None
        }
    }
}