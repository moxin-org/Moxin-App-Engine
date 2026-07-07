//! 预置 Prompt 模板库
//! Preset Prompt Template Library
//!
//! 提供常用场景的 Prompt 模板
//! Provides Prompt templates for common scenarios

use super::registry::PromptRegistry;
use super::template::{PromptTemplate, PromptVariable};

// ============================================================================
// 通用助手模板
// General Assistant Templates
// ============================================================================

/// 通用助手系统提示
/// General assistant system prompt
pub fn general_assistant() -> PromptTemplate {
    PromptTemplate::new("general-assistant")
        .with_name("通用助手")
        // General Assistant
        .with_description("通用 AI 助手系统提示")
        // General AI assistant system prompt
        .with_content(
            "你是一个乐于助人的 AI 助手。请以清晰、准确、专业的方式回答用户的问题。\n\n\
            回答时请注意：\n\
            1. 保持回答简洁明了\n\
            2. 如果不确定，请诚实说明\n\
            3. 必要时提供示例说明",
        )
        .with_tag("system")
        .with_tag("general")
}

/// 专业角色助手
/// Professional role assistant
pub fn role_assistant() -> PromptTemplate {
    PromptTemplate::new("role-assistant")
        .with_name("角色助手")
        // Role Assistant
        .with_description("可自定义角色的助手模板")
        // Assistant template with customizable roles
        .with_content(
            "你是一个专业的{role}。你的专长是{expertise}。\n\n\
            在回答问题时，请：\n\
            1. 运用你的专业知识\n\
            2. 提供有见地的分析\n\
            3. 给出可操作的建议",
        )
        .with_variable(
            PromptVariable::new("role").with_description("角色名称，如：软件工程师、数据分析师"),
            // Role name, e.g., Software Engineer, Data Analyst
        )
        .with_variable(
            PromptVariable::new("expertise")
                .with_description("专业领域")
                // Professional field
                .with_default("解决问题和提供帮助"),
            // Problem solving and providing help
        )
        .with_tag("system")
        .with_tag("role")
}

// ============================================================================
// 代码相关模板
// Code Related Templates
// ============================================================================

/// 代码审查模板
/// Code review template
pub fn code_review() -> PromptTemplate {
    PromptTemplate::new("code-review")
        .with_name("代码审查")
        // Code Review
        .with_description("专业代码审查模板")
        // Professional code review template
        .with_content(
            "请作为一个资深的{language}开发者，审查以下代码：\n\n\
            ```{language}\n{code}\n```\n\n\
            请从以下方面进行审查：\n\
            1. **代码质量**：可读性、命名规范、代码结构\n\
            2. **潜在问题**：Bug、边界情况、错误处理\n\
            3. **性能**：效率问题、优化建议\n\
            4. **安全性**：安全漏洞、敏感信息处理\n\
            5. **最佳实践**：设计模式、惯用写法\n\n\
            请提供具体的改进建议和示例代码。",
        )
        .with_variable(
            PromptVariable::new("language")
                .with_description("编程语言")
                // Programming language
                .with_default("代码"),
            // Code
        )
        .with_variable(PromptVariable::new("code").with_description("要审查的代码"))
        // The code to be reviewed
        .with_tag("code")
        .with_tag("review")
}

/// 代码解释模板
/// Code explanation template
pub fn code_explain() -> PromptTemplate {
    PromptTemplate::new("code-explain")
        .with_name("代码解释")
        // Code Explanation
        .with_description("解释代码功能和原理")
        // Explain code functionality and principles
        .with_content(
            "请详细解释以下{language}代码的功能和工作原理：\n\n\
            ```{language}\n{code}\n```\n\n\
            请包含：\n\
            1. **功能概述**：代码的主要用途\n\
            2. **逐行/逐块解释**：关键部分的详细说明\n\
            3. **使用示例**：如何调用或使用这段代码\n\
            4. **注意事项**：使用时需要注意的问题",
        )
        .with_variable(
            PromptVariable::new("language")
                .with_description("编程语言")
                // Programming language
                .with_default("代码"),
            // Code
        )
        .with_variable(PromptVariable::new("code").with_description("要解释的代码"))
        // The code to be explained
        .with_tag("code")
        .with_tag("explain")
}

/// 代码生成模板
/// Code generation template
pub fn code_generate() -> PromptTemplate {
    PromptTemplate::new("code-generate")
        .with_name("代码生成")
        // Code Generation
        .with_description("根据需求生成代码")
        // Generate code based on requirements
        .with_content(
            "请使用 {language} 编写代码实现以下功能：\n\n\
            **需求描述**：\n{requirement}\n\n\
            **要求**：\n\
            1. 代码应该简洁、高效、可读性强\n\
            2. 包含必要的注释说明\n\
            3. 考虑边界情况和错误处理\n\
            4. 遵循 {language} 的最佳实践",
        )
        .with_variable(PromptVariable::new("language").with_description("编程语言"))
        // Programming language
        .with_variable(PromptVariable::new("requirement").with_description("功能需求描述"))
        // Functional requirement description
        .with_tag("code")
        .with_tag("generate")
}

/// 代码重构模板
/// Code refactoring template
pub fn code_refactor() -> PromptTemplate {
    PromptTemplate::new("code-refactor")
        .with_name("代码重构")
        // Code Refactor
        .with_description("重构和优化代码")
        // Refactor and optimize code
        .with_content(
            "请重构以下{language}代码，{goal}：\n\n\
            ```{language}\n{code}\n```\n\n\
            重构时请：\n\
            1. 保持功能不变\n\
            2. 提高代码质量\n\
            3. 解释你的改动及原因\n\
            4. 提供重构后的完整代码",
        )
        .with_variable(
            PromptVariable::new("language")
                .with_description("编程语言")
                // Programming language
                .with_default("代码"),
            // Code
        )
        .with_variable(PromptVariable::new("code").with_description("要重构的代码"))
        // The code to be refactored
        .with_variable(
            PromptVariable::new("goal")
                .with_description("重构目标")
                // Refactoring goal
                .with_default("使其更加清晰、高效"),
            // Make it clearer and more efficient
        )
        .with_tag("code")
        .with_tag("refactor")
}

/// 单元测试生成模板
/// Unit test generation template
pub fn code_test() -> PromptTemplate {
    PromptTemplate::new("code-test")
        .with_name("测试生成")
        // Test Generation
        .with_description("为代码生成单元测试")
        // Generate unit tests for code
        .with_content(
            "请为以下{language}代码编写单元测试：\n\n\
            ```{language}\n{code}\n```\n\n\
            测试要求：\n\
            1. 覆盖主要功能路径\n\
            2. 包含边界情况测试\n\
            3. 包含错误情况测试\n\
            4. 使用 {test_framework} 测试框架",
        )
        .with_variable(PromptVariable::new("language").with_description("编程语言"))
        // Programming language
        .with_variable(PromptVariable::new("code").with_description("要测试的代码"))
        // The code to be tested
        .with_variable(
            PromptVariable::new("test_framework")
                .with_description("测试框架")
                // Test framework
                .with_default("标准"),
            // Standard
        )
        .with_tag("code")
        .with_tag("test")
}

// ============================================================================
// 写作和文档模板
// Writing and Documentation Templates
// ============================================================================

/// 技术文档模板
/// Technical documentation template
pub fn tech_doc() -> PromptTemplate {
    PromptTemplate::new("tech-doc")
        .with_name("技术文档")
        // Technical Documentation
        .with_description("撰写技术文档")
        // Writing technical documentation
        .with_content(
            "请为 {subject} 撰写技术文档。\n\n\
            文档应包含：\n\
            1. **概述**：简要介绍主题\n\
            2. **安装/配置**：如何开始使用\n\
            3. **使用指南**：基本用法和示例\n\
            4. **API 参考**：详细接口说明（如适用）\n\
            5. **常见问题**：FAQ 和故障排除\n\n\
            目标受众：{audience}",
        )
        .with_variable(PromptVariable::new("subject").with_description("文档主题"))
        // Documentation subject
        .with_variable(
            PromptVariable::new("audience")
                .with_description("目标读者")
                // Target audience
                .with_default("开发者"),
            // Developers
        )
        .with_tag("doc")
        .with_tag("writing")
}

/// 总结模板
/// Summarization template
pub fn summarize() -> PromptTemplate {
    PromptTemplate::new("summarize")
        .with_name("内容总结")
        // Content Summary
        .with_description("总结长文本内容")
        // Summarize long text content
        .with_content(
            "请总结以下内容：\n\n{content}\n\n\
            总结要求：\n\
            1. 长度约 {length}\n\
            2. 保留关键信息和主要观点\n\
            3. 使用清晰的结构组织\n\
            4. 语言简练准确",
        )
        .with_variable(PromptVariable::new("content").with_description("要总结的内容"))
        // The content to be summarized
        .with_variable(
            PromptVariable::new("length")
                .with_description("目标长度")
                // Target length
                .with_default("200-300字"),
            // 200-300 words
        )
        .with_tag("writing")
        .with_tag("summary")
}

/// 翻译模板
/// Translation template
pub fn translate() -> PromptTemplate {
    PromptTemplate::new("translate")
        .with_name("翻译")
        // Translation
        .with_description("翻译文本")
        // Translate text
        .with_content(
            "请将以下内容从{source_lang}翻译成{target_lang}：\n\n\
            {content}\n\n\
            翻译要求：\n\
            1. 保持原文含义准确\n\
            2. 符合目标语言的表达习惯\n\
            3. 保持专业术语的准确性\n\
            4. 保持原文的语气和风格",
        )
        .with_variable(
            PromptVariable::new("source_lang")
                .with_description("源语言")
                // Source language
                .with_default("英文"),
            // English
        )
        .with_variable(
            PromptVariable::new("target_lang")
                .with_description("目标语言")
                // Target language
                .with_default("中文"),
            // Chinese
        )
        .with_variable(PromptVariable::new("content").with_description("要翻译的内容"))
        // The content to be translated
        .with_tag("writing")
        .with_tag("translation")
}

// ============================================================================
// 分析和推理模板
// Analysis and Reasoning Templates
// ============================================================================

/// 问题分析模板
/// Problem analysis template
pub fn analyze() -> PromptTemplate {
    PromptTemplate::new("analyze")
        .with_name("问题分析")
        // Problem Analysis
        .with_description("分析问题并给出解决方案")
        // Analyze problems and provide solutions
        .with_content(
            "请分析以下问题并给出解决方案：\n\n\
            **问题描述**：\n{problem}\n\n\
            **上下文**：\n{context}\n\n\
            请提供：\n\
            1. **问题分析**：问题的根本原因\n\
            2. **解决方案**：可行的解决方法\n\
            3. **优劣分析**：各方案的优缺点\n\
            4. **推荐方案**：最佳建议及理由",
        )
        .with_variable(PromptVariable::new("problem").with_description("问题描述"))
        // Problem description
        .with_variable(
            PromptVariable::new("context")
                .with_description("相关背景信息")
                // Relevant background information
                .with_default("无额外上下文"),
            // No extra context
        )
        .with_tag("analysis")
        .with_tag("problem-solving")
}

/// 对比分析模板
/// Comparative analysis template
pub fn compare() -> PromptTemplate {
    PromptTemplate::new("compare")
        .with_name("对比分析")
        // Comparative Analysis
        .with_description("对比多个选项")
        // Compare multiple options
        .with_content(
            "请对比分析以下选项：\n\n\
            {options}\n\n\
            对比维度：{dimensions}\n\n\
            请提供：\n\
            1. **详细对比表格**\n\
            2. **各选项优缺点**\n\
            3. **适用场景分析**\n\
            4. **推荐建议**",
        )
        .with_variable(PromptVariable::new("options").with_description("要对比的选项列表"))
        // List of options to compare
        .with_variable(
            PromptVariable::new("dimensions")
                .with_description("对比维度")
                // Comparison dimensions
                .with_default("功能、性能、易用性、成本"),
            // Function, performance, usability, cost
        )
        .with_tag("analysis")
        .with_tag("comparison")
}

// ============================================================================
// ReAct Agent 模板
// ReAct Agent Templates
// ============================================================================

/// ReAct 推理系统提示
/// ReAct reasoning system prompt
pub fn react_system() -> PromptTemplate {
    PromptTemplate::new("react-system")
        .with_name("ReAct 系统提示")
        // ReAct System Prompt
        .with_description("ReAct Agent 的系统提示")
        // System prompt for ReAct Agent
        .with_content(
            "你是一个使用 ReAct（Reasoning + Acting）方法解决问题的 AI Agent。\n\n\
            你可以使用以下工具：\n{tools}\n\n\
            解决问题时，请遵循以下步骤：\n\n\
            1. **Thought（思考）**：分析当前情况，决定下一步行动\n\
            2. **Action（行动）**：选择合适的工具并执行\n\
            3. **Observation（观察）**：分析工具返回的结果\n\
            4. 重复上述步骤直到得到答案\n\
            5. **Final Answer（最终答案）**：给出完整的回答\n\n\
            格式要求：\n\
            - Thought: 你的思考过程\n\
            - Action: 工具名称[参数]\n\
            - Observation: 工具返回的结果\n\
            - Final Answer: 你的最终回答",
        )
        .with_variable(PromptVariable::new("tools").with_description("可用工具列表"))
        // List of available tools
        .with_tag("react")
        .with_tag("agent")
}

/// ReAct 任务模板
/// ReAct task template
pub fn react_task() -> PromptTemplate {
    PromptTemplate::new("react-task")
        .with_name("ReAct 任务")
        // ReAct Task
        .with_description("ReAct Agent 的任务模板")
        // Task template for ReAct Agent
        .with_content("请完成以下任务：\n\n{task}\n\n开始你的推理和行动：")
        .with_variable(PromptVariable::new("task").with_description("任务描述"))
        // Task description
        .with_tag("react")
        .with_tag("agent")
}

// ============================================================================
// 多 Agent 协作模板
// Multi-Agent Collaboration Templates
// ============================================================================

/// 辩论者模板
/// Debater template
pub fn debater() -> PromptTemplate {
    PromptTemplate::new("debater")
        .with_name("辩论者")
        // Debater
        .with_description("辩论模式中的辩论者角色")
        // Debater role in debate mode
        .with_content(
            "你是辩论中的{position}方。\n\n\
            辩题：{topic}\n\n\
            前述观点：\n{previous}\n\n\
            请从你的立场出发，提供有力的论点。注意：\n\
            1. 基于事实和逻辑论证\n\
            2. 回应对方的观点\n\
            3. 保持专业和理性",
        )
        .with_variable(
            PromptVariable::new("position")
                .with_description("辩论立场")
                // Debate position
                .with_enum(vec!["正".to_string(), "反".to_string()]),
            // Pro, Con
        )
        .with_variable(PromptVariable::new("topic").with_description("辩论话题"))
        // Debate topic
        .with_variable(
            PromptVariable::new("previous")
                .with_description("之前的辩论内容")
                // Previous debate content
                .with_default("这是辩论的开始"),
            // This is the beginning of the debate
        )
        .with_tag("multi-agent")
        .with_tag("debate")
}

/// 监督者模板
/// Supervisor template
pub fn supervisor() -> PromptTemplate {
    PromptTemplate::new("supervisor")
        .with_name("监督者")
        // Supervisor
        .with_description("监督模式中的监督者角色")
        // Supervisor role in supervision mode
        .with_content(
            "你是一个团队监督者，负责评估团队成员的工作成果。\n\n\
            **任务**：{task}\n\n\
            **团队成员的回答**：\n{responses}\n\n\
            请：\n\
            1. 评估每个回答的质量\n\
            2. 指出各自的优点和不足\n\
            3. 综合最佳内容给出最终答案\n\
            4. 给出改进建议",
        )
        .with_variable(PromptVariable::new("task").with_description("任务描述"))
        // Task description
        .with_variable(PromptVariable::new("responses").with_description("团队成员的回答"))
        // Team members' responses
        .with_tag("multi-agent")
        .with_tag("supervisor")
}

/// 聚合者模板
/// Aggregator template
pub fn aggregator() -> PromptTemplate {
    PromptTemplate::new("aggregator")
        .with_name("聚合者")
        // Aggregator
        .with_description("并行模式中的结果聚合角色")
        // Result aggregation role in parallel mode
        .with_content(
            "多个 Agent 已经分别处理了以下任务：\n\n\
            **原始任务**：{task}\n\n\
            **各 Agent 的结果**：\n{results}\n\n\
            请综合以上结果：\n\
            1. 找出共同点和差异点\n\
            2. 整合各方优势\n\
            3. 形成一个完整、准确的最终答案",
        )
        .with_variable(PromptVariable::new("task").with_description("原始任务"))
        // Original task
        .with_variable(PromptVariable::new("results").with_description("各 Agent 的结果"))
        // Results from each Agent
        .with_tag("multi-agent")
        .with_tag("aggregation")
}

// ============================================================================
// 注册表预加载
// Registry Preloading
// ============================================================================

/// 创建包含所有预置模板的注册中心
/// Creates a registry containing all preset templates
pub fn create_preset_registry() -> PromptRegistry {
    let mut registry = PromptRegistry::new();

    // 通用助手
    // General assistant
    registry.register(general_assistant());
    registry.register(role_assistant());

    // 代码相关
    // Code related
    registry.register(code_review());
    registry.register(code_explain());
    registry.register(code_generate());
    registry.register(code_refactor());
    registry.register(code_test());

    // 写作和文档
    // Writing and documentation
    registry.register(tech_doc());
    registry.register(summarize());
    registry.register(translate());

    // 分析和推理
    // Analysis and reasoning
    registry.register(analyze());
    registry.register(compare());

    // ReAct Agent
    registry.register(react_system());
    registry.register(react_task());

    // 多 Agent 协作
    // Multi-Agent collaboration
    registry.register(debater());
    registry.register(supervisor());
    registry.register(aggregator());

    registry
}

/// 将预置模板加载到现有注册中心
/// Loads preset templates into an existing registry
pub fn load_presets(registry: &mut PromptRegistry) {
    let presets = create_preset_registry();
    registry.merge(presets);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_templates() {
        let registry = create_preset_registry();

        // 验证模板存在
        // Verify template exists
        assert!(registry.contains("general-assistant"));
        assert!(registry.contains("code-review"));
        assert!(registry.contains("react-system"));
        assert!(registry.contains("supervisor"));

        // 验证标签
        // Verify tags
        let code_templates = registry.find_by_tag("code");
        assert!(code_templates.len() >= 4);

        let agent_templates = registry.find_by_tag("agent");
        assert!(agent_templates.len() >= 2);
    }

    #[test]
    fn test_code_review_template() {
        let template = code_review();

        let result = template
            .render(&[
                ("language", "rust"),
                ("code", "fn main() { info!(\"Hello\"); }"),
            ])
            .unwrap();

        assert!(result.contains("rust"));
        assert!(result.contains("fn main"));
    }

    #[test]
    fn test_role_assistant_template() {
        let template = role_assistant();

        // 使用默认值
        // Use default value
        let result = template.render(&[("role", "数据分析师")]).unwrap();

        assert!(result.contains("数据分析师"));
        assert!(result.contains("解决问题和提供帮助")); // 默认值
        // Default value
    }

    #[test]
    fn test_react_system_template() {
        let template = react_system();

        let tools = "1. search: 搜索网页\n2. calculator: 计算器";
        let result = template.render(&[("tools", tools)]).unwrap();

        assert!(result.contains("ReAct"));
        assert!(result.contains("search"));
        assert!(result.contains("Thought"));
    }

    #[test]
    fn test_translate_template() {
        let template = translate();

        let result = template.render(&[("content", "Hello, World!")]).unwrap();

        assert!(result.contains("Hello, World!"));
        assert!(result.contains("英文")); // 默认值
        // Default value
        assert!(result.contains("中文")); // 默认值
        // Default value
    }

    #[test]
    fn test_load_presets() {
        let mut registry = PromptRegistry::new();
        assert!(registry.is_empty());

        load_presets(&mut registry);

        assert!(!registry.is_empty());
        assert!(registry.len() >= 15);
    }
}
