use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

/// Strips method and constructor bodies from a Java source file,
/// preserving packages, imports, annotations, fields, and signatures.
pub fn extract_java_skeleton(source: &str) -> Result<String, String> {
    let language = tree_sitter_java::LANGUAGE.into();

    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|e| format!("Failed to load Java grammar: {e}"))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "Tree-sitter failed to parse Java source".to_string())?;

    // Capture the body blocks of all methods and constructors
    let query_src = r#"
        (method_declaration
            body: (block) @body)
        (constructor_declaration
            body: (constructor_body) @body)
    "#;

    let query =
        Query::new(&language, query_src).map_err(|e| format!("Invalid Tree-sitter query: {e}"))?;

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let mut ranges: Vec<(usize, usize)> = Vec::new();
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let node = capture.node;
            ranges.push((node.start_byte(), node.end_byte()));
        }
    }

    // Sort by start_byte ascending, then end_byte descending
    ranges.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)));

    // Filter out nested ranges (e.g., an anonymous inner class method inside a method body)
    let mut top_level_ranges: Vec<(usize, usize)> = Vec::new();
    for range in ranges {
        if let Some(last) = top_level_ranges.last() {
            if range.0 >= last.0 && range.1 <= last.1 {
                continue;
            }
        }
        top_level_ranges.push(range);
    }

    // Replace from the end of the file backward so earlier byte offsets stay valid
    let mut skeleton = source.to_string();
    for (start, end) in top_level_ranges.into_iter().rev() {
        skeleton.replace_range(start..end, "{ /* ... */ }");
    }

    println!("{skeleton}");

    Ok(skeleton)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_method_and_constructor_bodies_while_keeping_signatures() {
        let java_source = r#"
package com.example.payments;

import java.math.BigDecimal;
import java.util.Objects;

@Service
public class PaymentProcessor {
    private final PaymentGateway gateway;

    public PaymentProcessor(PaymentGateway gateway) {
        this.gateway = Objects.requireNonNull(gateway, "gateway must not be null");
    }

    public Receipt processPayment(String currency, BigDecimal amount) throws PaymentException {
        if (currency == null || currency.isBlank()) {
            throw new IllegalArgumentException("Currency required");
        }
        Runnable auditTask = new Runnable() {
            @Override
            public void run() {
                System.out.println("Nested anonymous class body");
            }
        };
        auditTask.run();
        return gateway.charge(currency, amount);
    }
}
"#;

        let skeleton = extract_java_skeleton(java_source).expect("AST extraction failed");

        // Preserved API boundaries
        assert!(skeleton.contains("package com.example.payments;"));
        assert!(skeleton.contains("import java.math.BigDecimal;"));
        assert!(skeleton.contains("@Service"));
        assert!(skeleton.contains("private final PaymentGateway gateway;"));
        assert!(skeleton.contains("public PaymentProcessor(PaymentGateway gateway) { /* ... */ }"));
        assert!(skeleton.contains(
            "public Receipt processPayment(String currency, BigDecimal amount) throws PaymentException { /* ... */ }"
        ));

        // Stripped implementation details
        assert!(!skeleton.contains("Objects.requireNonNull"));
        assert!(!skeleton.contains("IllegalArgumentException"));
        assert!(!skeleton.contains("Nested anonymous class body"));
        assert!(!skeleton.contains("gateway.charge"));
    }
}
