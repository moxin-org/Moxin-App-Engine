use mofa_plugins::{PdfTool, ToolExecutor};
use serde_json::json;
use std::path::Path;

#[tokio::test]
async fn test_pdf_tool_extracts_text() {
    let pdf_path = "tests/pdf/demo.pdf";

    // Ensure test PDF exists
    assert!(Path::new(pdf_path).exists());

    let tool = PdfTool::new();

    let args = json!({
        "pdf_path": pdf_path,
        "by_pages": false
    });

    let result = tool.execute(args).await;

    assert!(result.is_ok());

    let output = result.unwrap();

    println!("------ PDF TEXT OUTPUT ------");
    println!("{}", output["text"]);
    println!("-----------------------------");

    assert!(output["success"].as_bool().unwrap());
}
