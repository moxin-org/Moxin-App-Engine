//! Agent Skills 模块
//! Agent Skills Module
//!
//! 实现渐进式披露的 Agent Skills 系统，支持 SKILL.md 格式的技能定义。
//! Implements a progressive disclosure Agent Skills system, supporting SKILL.md format skill definitions.

pub mod disclosure;
pub mod metadata;
pub mod parser;
pub mod signature;

pub use disclosure::DisclosureController;
pub use metadata::{
    CodeFile, Requirement, RequirementCheck, SkillMetadata, SkillRequirements, SkillState,
    SkillVersion,
};
pub use parser::SkillParser;
pub use signature::TrustStore;
