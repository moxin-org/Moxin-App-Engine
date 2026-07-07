use mofa_plugins::{OcrTool, ToolExecutor};
use serde_json::json;

#[tokio::test]
async fn test_ocr_tool_runs_if_tesseract_exists() {
    if which::which("tesseract").is_err() {
        println!("Skipping OCR test because tesseract is not installed");
        return;
    }

    let img_path = "tests/image/image.png";

    assert!(std::path::Path::new(img_path).exists());

    let tool = OcrTool::new();

    let args = json!({
        "image_path": img_path,
        "lang": "eng"
    });

    let result = tool.execute(args).await;

    assert!(result.is_ok());

    let output = result.unwrap();

    println!("------ OCR OUTPUT ------");
    println!("{}", output["text"]);
    println!("------------------------");

    assert!(output["success"].as_bool().unwrap());
}
