fn main() {
    let text = std::fs::read_to_string(r"F:\svg3dx\samples\draft-0.1-sweep.svg3dx").unwrap();
    let result = svg3dx_core::parse_and_evaluate(&text).unwrap();
    dump_all(&result.instructions, 0);
    for w in &result.warnings { println!("WARN {w}"); }
}
fn dump_all(insts: &[svg3dx_core::GeometryInstruction], depth: usize) {
    use svg3dx_core::GeometryInstruction::*;
    let pad = "  ".repeat(depth);
    for (i, inst) in insts.iter().enumerate() {
        match inst {
            Primitive { kind, params, .. } => {
                println!("{pad}[{i}] {:?} corner={:?} r={:?}", kind, params.get("corner"), params.get("cornerRadius"));
                if let Some(p) = params.get("path") { println!("{pad}  path={p:?}"); }
            }
            Group { children, .. } => {
                println!("{pad}[{i}] Group n={}", children.len());
                dump_all(children, depth+1);
            }
            other => println!("{pad}[{i}] {:?}", other),
        }
    }
}
