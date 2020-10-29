use std::panic::{catch_unwind, AssertUnwindSafe};
use rusterators::generators::{BoostedGenerator, GeneratorChannel, ResultingGenerator};
fn main() {
    let mut g=BoostedGenerator::new(|g| {
        g.yield_val(0);
        g.yield_from(BoostedGenerator::new(|c| unimplemented!()))
    });
    let catch_result=catch_unwind(AssertUnwindSafe (|| {
        for i in &mut g {
            println!("value {}", i)
        }
    }));

    println!("result:  {:?} {:?}",catch_result,g.result());
}