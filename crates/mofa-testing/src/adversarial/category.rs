use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OwaspCategory {
    LLM01PromptInjection,
    LLM02InsecureOutputHandling,
    LLM03TrainingDataPoisoning,
    LLM04ModelDenialOfService,
    LLM05SupplyChainVulnerabilities,
    LLM06SensitiveInfoDisclosure,
    LLM07InsecurePluginDesign,
    LLM08ExcessiveAgency,
    LLM09Overreliance,
    LLM10ModelTheft,
}

impl OwaspCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LLM01PromptInjection => "LLM01: Prompt Injection",
            Self::LLM02InsecureOutputHandling => "LLM02: Insecure Output Handling",
            Self::LLM03TrainingDataPoisoning => "LLM03: Training Data Poisoning",
            Self::LLM04ModelDenialOfService => "LLM04: Model Denial of Service",
            Self::LLM05SupplyChainVulnerabilities => "LLM05: Supply Chain Vulnerabilities",
            Self::LLM06SensitiveInfoDisclosure => "LLM06: Sensitive Info Disclosure",
            Self::LLM07InsecurePluginDesign => "LLM07: Insecure Plugin Design",
            Self::LLM08ExcessiveAgency => "LLM08: Excessive Agency",
            Self::LLM09Overreliance => "LLM09: Overreliance",
            Self::LLM10ModelTheft => "LLM10: Model Theft",
        }
    }
}
