fn source(value: String) -> String {
    value
}

fn transform(value: String) -> String {
    value
}

fn sink(value: String) {
    println!("{value}");
}

fn main(user: String) {
    let raw = source(user);
    let clean = transform(raw);
    sink(clean);
}
