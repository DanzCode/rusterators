use std::panic::{catch_unwind, AssertUnwindSafe};
use rusterators::generators::Generator;
fn main() {
    let mut g=Generator::new(|g| {
        g.yield_val(0);
        g.yield_from::<Option<String>>(unimplemented!("insert recursive generator"))
    });
    let catch_result=catch_unwind(AssertUnwindSafe (|| {
        for i in &mut g {
            println!("value {}", i)
        }
    }));

    println!("result:  {:?} {:?}",catch_result,g.result());
}