use rusterators::generators::{BoostedGenerator, GeneratorChannel, Generator, ResultingGenerator};


fn create_line_generator<'a>(file_content: Result<String, String>) -> BoostedGenerator<'a, String, Result<(), String>, ()> {
    BoostedGenerator::new(move |g| {
        match &file_content {
            Ok(content) => {
                g.yield_all(content.lines().map(|s| String::from(s.trim())));
                Ok(())
            }
            Err(_) => {
                Err(String::from("failed to read lines"))
            }
        }
    })
}

struct RefStr<'a>(&'a str);

fn main() {
    let mut gt = BoostedGenerator::new_receiving(|gc, mut i: RefStr| {
        let mut v = Vec::<&str>::new();
        for _ in 0..2 {
            v.push(i.0);
            i = gc.yield_val(0);
        }
        v.iter().map(|s| s.len()).fold(0, |a, b| a + b)
    });
    for s in "a b c".split_whitespace() {
        gt.resume(RefStr(s));
    }
    println!("{:?}", gt.result());


    let mut g = create_line_generator(Ok(String::from(r#"1 line
    2 line
    3 line
    4 line"#)));

    for s in &mut g {
        println!("{}", s)
    }

    println!("result: {:?}", g.result());


    let mut g = create_line_generator(Err("".into()));
    for s in &mut g {
        println!("never read: {}", s);
    }
    println!("result: {:?}", g.result());
}