use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct JUnitTestBuilder {
    package: String,
    target_class: String,
    test_class_name: String,
    imports: BTreeSet<String>,
    mocks: Vec<(String, String)>, // (TypeName, fieldName)
    test_methods: Vec<String>,
}

impl JUnitTestBuilder {
    pub fn new(package: impl Into<String>, target_class: impl Into<String>) -> Self {
        let target_class = target_class.into();
        let test_class_name = format!("{target_class}Test");

        let mut imports = BTreeSet::new();
        imports.insert("org.junit.jupiter.api.BeforeEach".to_string());
        imports.insert("org.junit.jupiter.api.Test".to_string());
        imports.insert("org.junit.jupiter.api.extension.ExtendWith".to_string());
        imports.insert("org.mockito.InjectMocks".to_string());
        imports.insert("org.mockito.Mock".to_string());
        imports.insert("org.mockito.junit.jupiter.MockitoExtension".to_string());
        imports.insert("static org.junit.jupiter.api.Assertions.*".to_string());
        imports.insert("static org.mockito.Mockito.*".to_string());

        Self {
            package: package.into(),
            target_class,
            test_class_name,
            imports,
            mocks: Vec::new(),
            test_methods: Vec::new(),
        }
    }

    pub fn add_import(&mut self, import_path: impl Into<String>) {
        let cleaned = import_path
            .into()
            .trim()
            .trim_start_matches("import ")
            .trim_end_matches(';')
            .to_string();

        if !cleaned.is_empty() {
            self.imports.insert(cleaned);
        }
    }

    pub fn add_mock(&mut self, type_name: impl Into<String>, field_name: impl Into<String>) {
        let entry = (type_name.into(), field_name.into());
        if !self.mocks.contains(&entry) {
            self.mocks.push(entry);
        }
    }

    /// Cleans LLM output (strips markdown fences) and appends a single @Test method.
    pub fn append_test_method(&mut self, raw_llm_output: &str) -> Result<(), String> {
        let cleaned = strip_markdown_fences(raw_llm_output);
        if cleaned.is_empty() {
            return Err("LLM returned an empty test method".to_string());
        }
        if !cleaned.contains("@Test") {
            return Err("Generated snippet does not contain a @Test annotation".to_string());
        }

        self.test_methods.push(cleaned);
        Ok(())
    }

    /// Renders the complete, compilable JUnit 5 class.
    /// Feed this output back into Qwen's prompt on each iteration for rolling context,
    /// and write it to disk when the loop finishes.
    pub fn render(&self) -> String {
        let mut out = String::new();

        if !self.package.is_empty() {
            out.push_str(&format!("package {};\n\n", self.package));
        }

        for imp in &self.imports {
            out.push_str(&format!("import {imp};\n"));
        }

        out.push_str("\n@ExtendWith(MockitoExtension.class)\n");
        out.push_str(&format!("class {} {{\n\n", self.test_class_name));

        for (type_name, field_name) in &self.mocks {
            out.push_str(&format!(
                "    @Mock\n    private {type_name} {field_name};\n\n"
            ));
        }

        let target_field = to_lower_camel(&self.target_class);
        out.push_str(&format!(
            "    @InjectMocks\n    private {} {};\n\n",
            self.target_class, target_field
        ));

        out.push_str(
            "    @BeforeEach\n    void setUp() {\n        // Shared setup if needed\n    }\n",
        );

        for method in &self.test_methods {
            out.push('\n');
            for line in method.lines() {
                if line.trim().is_empty() {
                    out.push('\n');
                } else {
                    out.push_str("    ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }

        out.push_str("}\n");
        out
    }
}

fn strip_markdown_fences(input: &str) -> String {
    let trimmed = input.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }

    let mut lines: Vec<&str> = trimmed.lines().collect();
    if !lines.is_empty() && lines[0].trim_start().starts_with("```") {
        lines.remove(0);
    }
    if let Some(last) = lines.last() {
        if last.trim() == "```" {
            lines.pop();
        }
    }
    lines.join("\n").trim().to_string()
}

fn to_lower_camel(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_rolling_junit5_class_and_strips_fences() {
        let mut builder = JUnitTestBuilder::new("com.example.payments", "PaymentProcessor");
        builder.add_import("java.math.BigDecimal");
        builder.add_mock("PaymentGateway", "paymentGateway");

        // Simulate Qwen wrapping the first method in markdown fences
        let test_1 = r#"```java
@Test
void processPayment_throwsWhenCurrencyIsNull() {
    assertThrows(IllegalArgumentException.class, () -> {
        paymentProcessor.processPayment(null, BigDecimal.TEN);
    });
}
```"#;

        // Simulate Qwen returning raw Java for the second method
        let test_2 = r#"@Test
void processPayment_delegatesToGateway() {
    paymentProcessor.processPayment("USD", BigDecimal.TEN);
    verify(paymentGateway).charge("USD", BigDecimal.TEN);
}"#;

        builder.append_test_method(test_1).expect("test_1 failed");
        builder.append_test_method(test_2).expect("test_2 failed");

        let rendered = builder.render();

        assert!(rendered.contains("package com.example.payments;"));
        assert!(rendered.contains("import java.math.BigDecimal;"));
        assert!(rendered.contains("@ExtendWith(MockitoExtension.class)"));
        assert!(rendered.contains("private PaymentGateway paymentGateway;"));
        assert!(rendered.contains("private PaymentProcessor paymentProcessor;"));
        assert!(rendered.contains("void processPayment_throwsWhenCurrencyIsNull()"));
        assert!(rendered.contains("void processPayment_delegatesToGateway()"));
        assert!(!rendered.contains("```"));
    }

    #[test]
    fn rejects_output_without_test_annotation() {
        let mut builder = JUnitTestBuilder::new("com.example", "Demo");
        let result = builder.append_test_method("public void notATest() {}");
        assert!(result.is_err());
    }
}
