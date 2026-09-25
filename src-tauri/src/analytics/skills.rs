//! Canonical skills, technologies, certifications, languages and soft
//! skills, with their known aliases.
//!
//! Normalization is deterministic: every alias maps to one canonical name
//! ("K8s", "Kubernetes clusters" → "Kubernetes"; "Postgres" → "PostgreSQL")
//! and a category. Related skills share a *family*, which lets the Profile
//! comparison report a partial match (AWS ↔ Azure) instead of a plain miss.
//! A *generic* entry ("Cloud computing") is satisfied by any member of its
//! family.

use std::{collections::HashMap, sync::OnceLock};

use crate::models::analytics::{RequirementCategory as Cat, RequirementKind as Kind};

pub struct Def {
    pub name: &'static str,
    pub kind: Kind,
    pub category: Cat,
    pub family: Option<&'static str>,
    pub generic: bool,
    pub aliases: &'static [&'static str],
    /// Aliases that are ordinary words too ("Go", "React", "Spring"): in prose
    /// they only count when written like the skill name (see [`scan`]).
    pub ambiguous: &'static [&'static str],
}

const fn def(
    name: &'static str,
    category: Cat,
    family: Option<&'static str>,
    aliases: &'static [&'static str],
) -> Def {
    Def {
        name,
        kind: Kind::Skill,
        category,
        family,
        generic: false,
        aliases,
        ambiguous: &[],
    }
}

const fn generic(
    name: &'static str,
    category: Cat,
    family: &'static str,
    aliases: &'static [&'static str],
) -> Def {
    Def {
        generic: true,
        ..def(name, category, Some(family), aliases)
    }
}

const fn amb(
    name: &'static str,
    category: Cat,
    family: Option<&'static str>,
    aliases: &'static [&'static str],
    ambiguous: &'static [&'static str],
) -> Def {
    Def {
        ambiguous,
        ..def(name, category, family, aliases)
    }
}

const fn cert(name: &'static str, family: &'static str, aliases: &'static [&'static str]) -> Def {
    Def {
        kind: Kind::Certification,
        ..def(name, Cat::Certifications, Some(family), aliases)
    }
}

const fn soft(name: &'static str, aliases: &'static [&'static str]) -> Def {
    Def {
        kind: Kind::SoftSkill,
        ..def(name, Cat::SoftSkills, None, aliases)
    }
}

use Cat::{
    AiMl as AI, CloudInfrastructure as CLOUD, DataEngineering as DATA, Databases as DB,
    Frameworks as FW, ProgrammingLanguages as LANG, TechnicalSkills as TECH,
};

/// The dictionary. Order does not matter; the longest alias wins in text.
pub static SKILLS: &[Def] = &[
    // ── Programming languages ──
    def("Python", LANG, None, &["python", "python3", "python 3"]),
    def(
        "Java",
        LANG,
        Some("jvm"),
        &["java", "java ee", "jakarta ee"],
    ),
    def("Kotlin", LANG, Some("jvm"), &["kotlin"]),
    def("Scala", LANG, Some("jvm"), &["scala"]),
    def(
        "JavaScript",
        LANG,
        Some("js"),
        &["javascript", "ecmascript", "es6", "vanilla js"],
    ),
    def("TypeScript", LANG, Some("js"), &["typescript"]),
    amb("Go", LANG, None, &["golang", "go lang"], &["go"]),
    def("Rust", LANG, None, &["rust", "rustlang"]),
    def("C++", LANG, None, &["c++", "cpp", "modern c++"]),
    amb("C", LANG, None, &["ansi c", "c programming"], &["c"]),
    def("C#", LANG, None, &["c#", "csharp", "c sharp"]),
    def("Ruby", LANG, None, &["ruby"]),
    def("PHP", LANG, None, &["php"]),
    amb("Swift", LANG, None, &["swiftui"], &["swift"]),
    def(
        "Objective-C",
        LANG,
        None,
        &["objective-c", "objective c", "objc"],
    ),
    amb(
        "R",
        LANG,
        None,
        &["r programming", "rstudio", "r language"],
        &["r"],
    ),
    amb(
        "Julia",
        LANG,
        None,
        &["julia lang", "julialang"],
        &["julia"],
    ),
    def("MATLAB", LANG, None, &["matlab"]),
    def("SQL", LANG, None, &["sql", "ansi sql", "sql queries"]),
    def(
        "Shell scripting",
        LANG,
        None,
        &[
            "bash",
            "shell scripting",
            "shell scripts",
            "shell",
            "zsh",
            "bash scripting",
        ],
    ),
    def("PowerShell", LANG, None, &["powershell"]),
    def("Elixir", LANG, None, &["elixir"]),
    def("Haskell", LANG, None, &["haskell"]),
    amb("Dart", LANG, None, &[], &["dart"]),
    def("Perl", LANG, None, &["perl"]),
    def("Lua", LANG, None, &["lua"]),
    def("Solidity", LANG, None, &["solidity"]),
    def("CUDA", LANG, None, &["cuda"]),
    def("Fortran", LANG, None, &["fortran"]),
    def("COBOL", LANG, None, &["cobol"]),
    def("Groovy", LANG, Some("jvm"), &["groovy"]),
    def("Clojure", LANG, Some("jvm"), &["clojure"]),
    // ── Frameworks & libraries ──
    amb(
        "React",
        FW,
        Some("frontend"),
        &["react.js", "reactjs", "react js"],
        &["react"],
    ),
    def("React Native", FW, Some("mobile"), &["react native"]),
    def("Angular", FW, Some("frontend"), &["angular", "angularjs"]),
    def(
        "Vue.js",
        FW,
        Some("frontend"),
        &["vue", "vue.js", "vuejs", "vue js", "nuxt"],
    ),
    def("Svelte", FW, Some("frontend"), &["svelte", "sveltekit"]),
    def("Next.js", FW, Some("frontend"), &["next.js", "nextjs"]),
    amb(
        "Node.js",
        FW,
        Some("js-backend"),
        &["node.js", "nodejs", "node js"],
        &["node"],
    ),
    amb(
        "Express",
        FW,
        Some("js-backend"),
        &["express.js", "expressjs"],
        &["express"],
    ),
    def("NestJS", FW, Some("js-backend"), &["nestjs", "nest.js"]),
    def(
        "Django",
        FW,
        Some("py-backend"),
        &["django", "django rest framework", "drf"],
    ),
    def("Flask", FW, Some("py-backend"), &["flask"]),
    def("FastAPI", FW, Some("py-backend"), &["fastapi", "fast api"]),
    def(
        "Spring Boot",
        FW,
        Some("jvm-backend"),
        &["spring boot", "springboot"],
    ),
    amb(
        "Spring",
        FW,
        Some("jvm-backend"),
        &["spring framework"],
        &["spring"],
    ),
    def(
        ".NET",
        FW,
        None,
        &[".net", "dotnet", "asp.net", ".net core", "asp.net core"],
    ),
    def(
        "Ruby on Rails",
        FW,
        None,
        &["ruby on rails", "rails", "ror"],
    ),
    def("Laravel", FW, None, &["laravel"]),
    def("Flutter", FW, Some("mobile"), &["flutter"]),
    def("pandas", FW, Some("py-data"), &["pandas"]),
    def("NumPy", FW, Some("py-data"), &["numpy"]),
    def("Polars", FW, Some("py-data"), &["polars"]),
    // ── Cloud & infrastructure ──
    def(
        "AWS",
        CLOUD,
        Some("cloud"),
        &["aws", "amazon web services", "ec2", "s3", "aws lambda"],
    ),
    def("Azure", CLOUD, Some("cloud"), &["azure", "microsoft azure"]),
    def(
        "Google Cloud",
        CLOUD,
        Some("cloud"),
        &["gcp", "google cloud", "google cloud platform"],
    ),
    generic(
        "Cloud computing",
        CLOUD,
        "cloud",
        &[
            "cloud",
            "cloud computing",
            "cloud platforms",
            "cloud platform",
            "cloud infrastructure",
            "public cloud",
            "cloud services",
            "cloud-native",
            "cloud native",
            "hyperscaler",
        ],
    ),
    def(
        "Kubernetes",
        CLOUD,
        Some("orchestration"),
        &[
            "kubernetes",
            "k8s",
            "kubernetes clusters",
            "container orchestration",
            "eks",
            "aks",
            "gke",
        ],
    ),
    def("OpenShift", CLOUD, Some("orchestration"), &["openshift"]),
    def(
        "Docker",
        CLOUD,
        Some("containers"),
        &["docker", "docker compose", "dockerfiles"],
    ),
    generic(
        "Containers",
        CLOUD,
        "containers",
        &[
            "containers",
            "containerization",
            "containerisation",
            "container technologies",
        ],
    ),
    def("Helm", CLOUD, Some("k8s-tools"), &["helm", "helm charts"]),
    def(
        "Terraform",
        CLOUD,
        Some("iac"),
        &["terraform", "terraform cloud", "opentofu"],
    ),
    def("Pulumi", CLOUD, Some("iac"), &["pulumi"]),
    def(
        "CloudFormation",
        CLOUD,
        Some("iac"),
        &["cloudformation", "aws cloudformation"],
    ),
    def("AWS CDK", CLOUD, Some("iac"), &["aws cdk", "cdk"]),
    def("Bicep", CLOUD, Some("iac"), &["bicep", "arm templates"]),
    generic(
        "Infrastructure as Code",
        CLOUD,
        "iac",
        &["infrastructure as code", "iac", "infrastructure-as-code"],
    ),
    def("Ansible", CLOUD, Some("config"), &["ansible"]),
    amb("Chef", CLOUD, Some("config"), &[], &["chef"]),
    amb("Puppet", CLOUD, Some("config"), &[], &["puppet"]),
    generic(
        "CI/CD",
        CLOUD,
        "ci",
        &[
            "ci/cd",
            "ci cd",
            "cicd",
            "continuous integration",
            "continuous delivery",
            "continuous deployment",
            "ci/cd pipelines",
            "build pipelines",
        ],
    ),
    def("Jenkins", CLOUD, Some("ci"), &["jenkins"]),
    def("GitHub Actions", CLOUD, Some("ci"), &["github actions"]),
    def(
        "GitLab CI",
        CLOUD,
        Some("ci"),
        &["gitlab ci", "gitlab ci/cd", "gitlab pipelines"],
    ),
    def(
        "Azure DevOps",
        CLOUD,
        Some("ci"),
        &["azure devops", "azure pipelines"],
    ),
    def(
        "Argo CD",
        CLOUD,
        Some("gitops"),
        &["argocd", "argo cd", "argo"],
    ),
    def("Flux", CLOUD, Some("gitops"), &["fluxcd", "flux cd"]),
    generic("GitOps", CLOUD, "gitops", &["gitops"]),
    def(
        "Linux",
        CLOUD,
        None,
        &[
            "linux",
            "unix",
            "ubuntu",
            "debian",
            "rhel",
            "red hat enterprise linux",
            "centos",
        ],
    ),
    def("Prometheus", CLOUD, Some("observability"), &["prometheus"]),
    def("Grafana", CLOUD, Some("observability"), &["grafana"]),
    def("Datadog", CLOUD, Some("observability"), &["datadog"]),
    def(
        "OpenTelemetry",
        CLOUD,
        Some("observability"),
        &["opentelemetry", "otel"],
    ),
    def(
        "ELK Stack",
        CLOUD,
        Some("observability"),
        &["elk", "elk stack", "kibana", "logstash"],
    ),
    def("Splunk", CLOUD, Some("observability"), &["splunk"]),
    generic(
        "Observability",
        CLOUD,
        "observability",
        &[
            "observability",
            "logging and monitoring",
            "monitoring and alerting",
            "monitoring and observability",
        ],
    ),
    def(
        "Serverless",
        CLOUD,
        Some("cloud"),
        &[
            "serverless",
            "lambda functions",
            "cloud functions",
            "azure functions",
        ],
    ),
    def("DevOps", CLOUD, None, &["devops", "dev ops"]),
    def(
        "SRE",
        CLOUD,
        None,
        &["sre", "site reliability engineering", "site reliability"],
    ),
    def("Istio", CLOUD, None, &["istio", "service mesh"]),
    def(
        "GPU infrastructure",
        CLOUD,
        None,
        &[
            "gpu",
            "gpus",
            "gpu clusters",
            "gpu computing",
            "gpu infrastructure",
        ],
    ),
    def(
        "HPC",
        CLOUD,
        None,
        &[
            "hpc",
            "high performance computing",
            "high-performance computing",
            "slurm",
        ],
    ),
    def(
        "Networking",
        TECH,
        None,
        &[
            "networking",
            "tcp/ip",
            "network protocols",
            "load balancing",
        ],
    ),
    // ── AI / ML ──
    generic(
        "Machine Learning",
        AI,
        "ml",
        &[
            "machine learning",
            "ml",
            "ml models",
            "machine learning models",
        ],
    ),
    def(
        "Deep Learning",
        AI,
        Some("ml"),
        &["deep learning", "neural networks", "neural network"],
    ),
    def(
        "NLP",
        AI,
        Some("ml"),
        &["nlp", "natural language processing", "text mining"],
    ),
    def(
        "Computer Vision",
        AI,
        Some("ml"),
        &[
            "computer vision",
            "image recognition",
            "object detection",
            "image processing",
        ],
    ),
    def(
        "LLMs",
        AI,
        Some("genai"),
        &[
            "llm",
            "llms",
            "large language models",
            "large language model",
            "foundation models",
            "language models",
        ],
    ),
    generic(
        "Generative AI",
        AI,
        "genai",
        &["generative ai", "genai", "gen ai", "generative models"],
    ),
    def(
        "RAG",
        AI,
        Some("genai"),
        &[
            "rag",
            "retrieval augmented generation",
            "retrieval-augmented generation",
            "retrieval augmented",
        ],
    ),
    def(
        "Prompt engineering",
        AI,
        Some("genai"),
        &["prompt engineering", "prompting", "prompt design"],
    ),
    def(
        "Fine-tuning",
        AI,
        Some("genai"),
        &[
            "fine-tuning",
            "fine tuning",
            "finetuning",
            "lora",
            "qlora",
            "peft",
        ],
    ),
    def(
        "AI agents",
        AI,
        Some("genai"),
        &[
            "ai agents",
            "agentic ai",
            "agentic systems",
            "llm agents",
            "agentic workflows",
            "multi-agent systems",
            "agent frameworks",
        ],
    ),
    def(
        "Transformers",
        AI,
        Some("ml"),
        &[
            "transformers",
            "transformer models",
            "transformer architectures",
            "bert",
        ],
    ),
    def(
        "Reinforcement Learning",
        AI,
        Some("ml"),
        &["reinforcement learning", "rlhf"],
    ),
    generic(
        "MLOps",
        AI,
        "mlops",
        &[
            "mlops",
            "ml ops",
            "machine learning operations",
            "model deployment",
            "model serving",
            "ml infrastructure",
            "ml platform",
        ],
    ),
    def("LLMOps", AI, Some("mlops"), &["llmops", "llm ops"]),
    def(
        "Embeddings",
        AI,
        Some("genai"),
        &["embeddings", "vector embeddings", "semantic search"],
    ),
    def(
        "Model evaluation",
        AI,
        Some("ml"),
        &[
            "llm evaluation",
            "evals",
            "model evaluation",
            "evaluation frameworks",
        ],
    ),
    def(
        "Statistics",
        AI,
        None,
        &[
            "statistics",
            "statistical modeling",
            "statistical modelling",
            "statistical analysis",
            "probability",
        ],
    ),
    def("Data science", AI, Some("ml"), &["data science"]),
    def(
        "Time series",
        AI,
        Some("ml"),
        &["time series", "time-series", "forecasting"],
    ),
    def(
        "Recommender systems",
        AI,
        Some("ml"),
        &[
            "recommender systems",
            "recommendation systems",
            "recommendation engines",
        ],
    ),
    def(
        "PyTorch",
        AI,
        Some("dl-framework"),
        &["pytorch", "torch", "pytorch lightning"],
    ),
    def(
        "TensorFlow",
        AI,
        Some("dl-framework"),
        &["tensorflow", "tf2"],
    ),
    def("JAX", AI, Some("dl-framework"), &["jax"]),
    def("Keras", AI, Some("dl-framework"), &["keras"]),
    def(
        "scikit-learn",
        AI,
        Some("ml-lib"),
        &["scikit-learn", "sklearn", "scikit learn"],
    ),
    def("XGBoost", AI, Some("ml-lib"), &["xgboost"]),
    def("LightGBM", AI, Some("ml-lib"), &["lightgbm"]),
    def(
        "Hugging Face",
        AI,
        None,
        &["hugging face", "huggingface", "hf transformers"],
    ),
    def("OpenCV", AI, None, &["opencv"]),
    def(
        "LangChain",
        AI,
        Some("llm-framework"),
        &["langchain", "lang chain"],
    ),
    def(
        "LangGraph",
        AI,
        Some("llm-framework"),
        &["langgraph", "lang graph"],
    ),
    def(
        "LlamaIndex",
        AI,
        Some("llm-framework"),
        &["llamaindex", "llama index", "llama-index"],
    ),
    def("Haystack", AI, Some("llm-framework"), &["haystack"]),
    def(
        "Semantic Kernel",
        AI,
        Some("llm-framework"),
        &["semantic kernel"],
    ),
    def("DSPy", AI, Some("llm-framework"), &["dspy"]),
    def("CrewAI", AI, Some("llm-framework"), &["crewai", "crew ai"]),
    def("AutoGen", AI, Some("llm-framework"), &["autogen"]),
    def(
        "OpenAI API",
        AI,
        Some("llm-api"),
        &["openai api", "openai", "gpt-4", "gpt-4o", "chatgpt api"],
    ),
    def("vLLM", AI, Some("llm-serving"), &["vllm"]),
    def(
        "NVIDIA Triton",
        AI,
        Some("llm-serving"),
        &["nvidia triton", "triton inference server", "triton"],
    ),
    def(
        "TensorRT",
        AI,
        Some("llm-serving"),
        &["tensorrt", "tensorrt-llm"],
    ),
    def(
        "Text Generation Inference",
        AI,
        Some("llm-serving"),
        &["text generation inference", "tgi"],
    ),
    def("KServe", AI, Some("llm-serving"), &["kserve", "kfserving"]),
    def("BentoML", AI, Some("llm-serving"), &["bentoml"]),
    amb(
        "Ray",
        AI,
        Some("distributed-ml"),
        &["ray serve", "ray tune", "ray train", "anyscale", "ray.io"],
        &["ray"],
    ),
    def("MLflow", AI, Some("mlops"), &["mlflow"]),
    def(
        "Kubeflow",
        AI,
        Some("mlops"),
        &["kubeflow", "kubeflow pipelines"],
    ),
    def(
        "Weights & Biases",
        AI,
        Some("mlops"),
        &["weights & biases", "weights and biases", "wandb"],
    ),
    def(
        "SageMaker",
        AI,
        Some("mlops"),
        &["sagemaker", "amazon sagemaker", "aws sagemaker"],
    ),
    def(
        "Vertex AI",
        AI,
        Some("mlops"),
        &["vertex ai", "vertexai", "google vertex ai"],
    ),
    def(
        "Azure Machine Learning",
        AI,
        Some("mlops"),
        &["azure ml", "azure machine learning", "azure ai"],
    ),
    // ── Data engineering ──
    def(
        "Apache Spark",
        DATA,
        Some("batch"),
        &[
            "spark",
            "apache spark",
            "pyspark",
            "spark sql",
            "spark streaming",
        ],
    ),
    def(
        "Hadoop",
        DATA,
        Some("batch"),
        &["hadoop", "hdfs", "mapreduce"],
    ),
    amb(
        "Hive",
        DATA,
        Some("batch"),
        &["apache hive", "hiveql"],
        &["hive"],
    ),
    amb(
        "Apache Beam",
        DATA,
        Some("batch"),
        &["apache beam"],
        &["beam"],
    ),
    def("Dask", DATA, Some("batch"), &["dask"]),
    def(
        "Apache Flink",
        DATA,
        Some("stream"),
        &["flink", "apache flink"],
    ),
    def(
        "Apache Kafka",
        DATA,
        Some("stream"),
        &["kafka", "apache kafka", "confluent", "kafka streams"],
    ),
    def("Kinesis", DATA, Some("stream"), &["kinesis", "aws kinesis"]),
    def(
        "Pub/Sub",
        DATA,
        Some("stream"),
        &["pub/sub", "pubsub", "google pub/sub"],
    ),
    def("RabbitMQ", DATA, Some("messaging"), &["rabbitmq"]),
    def(
        "Apache Airflow",
        DATA,
        Some("orchestrator"),
        &["airflow", "apache airflow", "mwaa"],
    ),
    def("Dagster", DATA, Some("orchestrator"), &["dagster"]),
    def("Prefect", DATA, Some("orchestrator"), &["prefect"]),
    def("dbt", DATA, None, &["dbt", "data build tool", "dbt core"]),
    def(
        "ETL",
        DATA,
        None,
        &[
            "etl",
            "elt",
            "etl/elt",
            "data pipelines",
            "data pipeline",
            "etl pipelines",
        ],
    ),
    def(
        "Data modeling",
        DATA,
        None,
        &[
            "data modeling",
            "data modelling",
            "dimensional modeling",
            "dimensional modelling",
        ],
    ),
    generic(
        "Data warehousing",
        DATA,
        "warehouse",
        &[
            "data warehouse",
            "data warehousing",
            "dwh",
            "data lake",
            "lakehouse",
            "data lakehouse",
        ],
    ),
    def("Snowflake", DATA, Some("warehouse"), &["snowflake"]),
    def(
        "BigQuery",
        DATA,
        Some("warehouse"),
        &["bigquery", "big query"],
    ),
    def(
        "Redshift",
        DATA,
        Some("warehouse"),
        &["redshift", "amazon redshift"],
    ),
    def(
        "Databricks",
        DATA,
        Some("warehouse"),
        &["databricks", "delta lake"],
    ),
    def(
        "Azure Synapse",
        DATA,
        Some("warehouse"),
        &["azure synapse", "synapse analytics"],
    ),
    def(
        "Data governance",
        DATA,
        None,
        &[
            "data governance",
            "data lineage",
            "data catalog",
            "data cataloging",
        ],
    ),
    def(
        "Data quality",
        DATA,
        None,
        &["data quality", "data validation", "great expectations"],
    ),
    def("Tableau", DATA, Some("bi"), &["tableau"]),
    def("Power BI", DATA, Some("bi"), &["power bi", "powerbi"]),
    def("Looker", DATA, Some("bi"), &["looker", "looker studio"]),
    amb(
        "Business intelligence",
        DATA,
        Some("bi"),
        &["business intelligence"],
        &["bi"],
    ),
    amb(
        "Excel",
        TECH,
        None,
        &["microsoft excel", "ms excel"],
        &["excel"],
    ),
    // ── Databases ──
    def(
        "PostgreSQL",
        DB,
        Some("sql-db"),
        &[
            "postgresql",
            "postgres",
            "psql",
            "postgre sql",
            "pgsql",
            "postgre",
        ],
    ),
    def("MySQL", DB, Some("sql-db"), &["mysql"]),
    def("MariaDB", DB, Some("sql-db"), &["mariadb"]),
    def(
        "SQL Server",
        DB,
        Some("sql-db"),
        &[
            "sql server",
            "mssql",
            "ms sql",
            "t-sql",
            "tsql",
            "microsoft sql server",
        ],
    ),
    def(
        "Oracle Database",
        DB,
        Some("sql-db"),
        &["oracle database", "oracle db", "pl/sql", "plsql"],
    ),
    def("SQLite", DB, Some("sql-db"), &["sqlite"]),
    generic(
        "Relational databases",
        DB,
        "sql-db",
        &[
            "relational databases",
            "relational database",
            "rdbms",
            "sql databases",
        ],
    ),
    def("MongoDB", DB, Some("nosql"), &["mongodb", "mongo"]),
    def("Cassandra", DB, Some("nosql"), &["cassandra", "scylladb"]),
    def("DynamoDB", DB, Some("nosql"), &["dynamodb"]),
    generic(
        "NoSQL",
        DB,
        "nosql",
        &["nosql", "nosql databases", "document databases"],
    ),
    def("Redis", DB, Some("kv"), &["redis", "memcached"]),
    def(
        "Elasticsearch",
        DB,
        Some("search"),
        &["elasticsearch", "elastic search"],
    ),
    def("OpenSearch", DB, Some("search"), &["opensearch"]),
    generic(
        "Vector databases",
        DB,
        "vector-db",
        &[
            "vector databases",
            "vector database",
            "vector db",
            "vector dbs",
            "vector stores",
            "vector store",
            "vector search",
        ],
    ),
    def("Pinecone", DB, Some("vector-db"), &["pinecone"]),
    def("Weaviate", DB, Some("vector-db"), &["weaviate"]),
    def("Milvus", DB, Some("vector-db"), &["milvus"]),
    def("Qdrant", DB, Some("vector-db"), &["qdrant"]),
    def("Chroma", DB, Some("vector-db"), &["chromadb", "chroma db"]),
    def("pgvector", DB, Some("vector-db"), &["pgvector"]),
    def("FAISS", DB, Some("vector-db"), &["faiss"]),
    def("Neo4j", DB, Some("graph-db"), &["neo4j", "cypher"]),
    generic(
        "Graph databases",
        DB,
        "graph-db",
        &[
            "graph databases",
            "graph database",
            "knowledge graphs",
            "knowledge graph",
        ],
    ),
    def("ClickHouse", DB, None, &["clickhouse"]),
    // ── General engineering ──
    def(
        "Git",
        TECH,
        None,
        &["git", "github", "gitlab", "bitbucket", "version control"],
    ),
    amb(
        "REST APIs",
        TECH,
        Some("api"),
        &[
            "rest api",
            "rest apis",
            "restful",
            "restful apis",
            "restful services",
            "rest services",
        ],
        &["rest"],
    ),
    def("GraphQL", TECH, Some("api"), &["graphql"]),
    def("gRPC", TECH, Some("api"), &["grpc"]),
    generic(
        "API design",
        TECH,
        "api",
        &[
            "api design",
            "api development",
            "designing apis",
            "building apis",
        ],
    ),
    def(
        "System design",
        TECH,
        None,
        &[
            "system design",
            "software architecture",
            "system architecture",
            "solution architecture",
        ],
    ),
    def(
        "Distributed systems",
        TECH,
        None,
        &["distributed systems", "distributed computing"],
    ),
    def(
        "Microservices",
        TECH,
        None,
        &[
            "microservices",
            "microservice architecture",
            "micro-services",
            "microservice",
        ],
    ),
    def(
        "Automated testing",
        TECH,
        None,
        &[
            "unit testing",
            "automated testing",
            "test automation",
            "tdd",
            "test-driven development",
            "integration testing",
        ],
    ),
    def(
        "Application security",
        TECH,
        None,
        &[
            "devsecops",
            "cybersecurity",
            "cyber security",
            "secure coding",
            "security best practices",
        ],
    ),
    def(
        "Agile",
        TECH,
        None,
        &[
            "agile",
            "agile methodologies",
            "agile development",
            "scrum",
            "kanban",
        ],
    ),
    def(
        "Algorithms & data structures",
        TECH,
        None,
        &[
            "algorithms",
            "data structures",
            "algorithms and data structures",
        ],
    ),
    def("Jira", TECH, None, &["jira", "confluence"]),
    // ── Certifications ──
    cert(
        "AWS Certified Solutions Architect",
        "aws-cert",
        &[
            "aws certified solutions architect",
            "aws solutions architect",
            "aws solutions architect associate",
            "saa-c03",
            "aws solution architect",
        ],
    ),
    cert(
        "AWS Certified Developer",
        "aws-cert",
        &["aws certified developer", "aws developer associate"],
    ),
    cert(
        "AWS Certified DevOps Engineer",
        "aws-cert",
        &[
            "aws certified devops engineer",
            "aws devops engineer professional",
        ],
    ),
    cert(
        "AWS Certified Machine Learning",
        "aws-cert",
        &[
            "aws certified machine learning",
            "aws machine learning specialty",
            "aws ml specialty",
        ],
    ),
    cert(
        "AWS Certified Data Engineer",
        "aws-cert",
        &["aws certified data engineer"],
    ),
    cert(
        "AWS certification",
        "aws-cert",
        &["aws certification", "aws certifications", "aws certified"],
    ),
    cert(
        "Azure Fundamentals (AZ-900)",
        "azure-cert",
        &["az-900", "azure fundamentals"],
    ),
    cert(
        "Azure Administrator (AZ-104)",
        "azure-cert",
        &["az-104", "azure administrator"],
    ),
    cert(
        "Azure Solutions Architect (AZ-305)",
        "azure-cert",
        &["az-305", "azure solutions architect"],
    ),
    cert(
        "Azure AI Engineer (AI-102)",
        "azure-cert",
        &["ai-102", "azure ai engineer"],
    ),
    cert(
        "Azure Data Scientist (DP-100)",
        "azure-cert",
        &["dp-100", "azure data scientist"],
    ),
    cert(
        "Azure Data Engineer (DP-203)",
        "azure-cert",
        &["dp-203", "azure data engineer"],
    ),
    cert(
        "Azure certification",
        "azure-cert",
        &[
            "azure certification",
            "azure certifications",
            "microsoft certified",
        ],
    ),
    cert(
        "Google Professional Cloud Architect",
        "gcp-cert",
        &["professional cloud architect", "google cloud architect"],
    ),
    cert(
        "Google Professional Data Engineer",
        "gcp-cert",
        &["professional data engineer", "google cloud data engineer"],
    ),
    cert(
        "Google Professional ML Engineer",
        "gcp-cert",
        &[
            "professional machine learning engineer",
            "google cloud ml engineer",
        ],
    ),
    cert(
        "CKA",
        "k8s-cert",
        &["cka", "certified kubernetes administrator"],
    ),
    cert(
        "CKAD",
        "k8s-cert",
        &["ckad", "certified kubernetes application developer"],
    ),
    cert(
        "CKS",
        "k8s-cert",
        &["cks", "certified kubernetes security specialist"],
    ),
    cert(
        "HashiCorp Terraform Associate",
        "iac-cert",
        &[
            "terraform associate",
            "hashicorp certified terraform associate",
            "hashicorp terraform associate",
        ],
    ),
    cert(
        "PMP",
        "pm-cert",
        &["pmp", "project management professional"],
    ),
    cert("PRINCE2", "pm-cert", &["prince2"]),
    cert(
        "Scrum Master certification",
        "agile-cert",
        &[
            "certified scrummaster",
            "certified scrum master",
            "csm",
            "psm",
            "professional scrum master",
        ],
    ),
    cert(
        "SAFe",
        "agile-cert",
        &["safe agilist", "scaled agile framework"],
    ),
    cert("ITIL", "itsm-cert", &["itil", "itil v4", "itil 4"]),
    cert("CISSP", "security-cert", &["cissp"]),
    cert(
        "CompTIA Security+",
        "security-cert",
        &["security+", "comptia security+", "comptia security"],
    ),
    cert("CISM", "security-cert", &["cism"]),
    cert("CCNA", "network-cert", &["ccna"]),
    cert("TOGAF", "architecture-cert", &["togaf"]),
    cert(
        "Databricks certification",
        "data-cert",
        &["databricks certified", "databricks certification"],
    ),
    cert(
        "SnowPro",
        "data-cert",
        &["snowpro", "snowflake certification"],
    ),
    cert(
        "TensorFlow Developer Certificate",
        "ml-cert",
        &["tensorflow developer certificate", "tensorflow certificate"],
    ),
    // ── Soft skills ──
    soft(
        "Communication",
        &[
            "communication skills",
            "communication",
            "communicative",
            "communicator",
            "kommunikationsstärke",
            "kommunikationsfähigkeit",
        ],
    ),
    soft(
        "Teamwork",
        &[
            "team player",
            "teamwork",
            "collaboration",
            "collaborative",
            "teamfähigkeit",
            "cross-functional collaboration",
        ],
    ),
    soft(
        "Problem solving",
        &[
            "problem solving",
            "problem-solving",
            "problem solver",
            "troubleshooting skills",
        ],
    ),
    soft(
        "Analytical thinking",
        &[
            "analytical thinking",
            "analytical skills",
            "analytical mindset",
            "analytisches denken",
        ],
    ),
    soft(
        "Leadership",
        &[
            "leadership",
            "leading teams",
            "team leadership",
            "people management",
            "führungserfahrung",
        ],
    ),
    soft(
        "Stakeholder management",
        &[
            "stakeholder management",
            "stakeholder communication",
            "working with stakeholders",
        ],
    ),
    soft("Mentoring", &["mentoring", "mentorship", "coaching"]),
    soft(
        "Ownership",
        &["ownership", "accountability", "sense of ownership"],
    ),
    soft(
        "Self-motivation",
        &[
            "self-motivated",
            "self motivated",
            "proactive",
            "self-starter",
            "self starter",
            "eigeninitiative",
        ],
    ),
    soft(
        "Presentation skills",
        &["presentation skills", "presenting", "public speaking"],
    ),
    soft("Adaptability", &["adaptability", "adaptable"]),
    soft(
        "Attention to detail",
        &["attention to detail", "detail-oriented", "detail oriented"],
    ),
    soft(
        "Customer focus",
        &[
            "customer focus",
            "customer-oriented",
            "customer orientation",
            "client-facing",
        ],
    ),
    soft("Critical thinking", &["critical thinking"]),
];

/// Spoken languages: canonical English name and the names used for them.
pub static LANGUAGES: &[(&str, &[&str])] = &[
    (
        "English",
        &["english", "englisch", "anglais", "inglés", "ingles"],
    ),
    (
        "German",
        &["german", "deutsch", "allemand", "alemán", "aleman"],
    ),
    (
        "French",
        &[
            "french",
            "französisch",
            "franzoesisch",
            "français",
            "francais",
        ],
    ),
    (
        "Spanish",
        &["spanish", "spanisch", "español", "espanol", "castellano"],
    ),
    ("Italian", &["italian", "italienisch", "italiano"]),
    (
        "Portuguese",
        &["portuguese", "portugiesisch", "português", "portugues"],
    ),
    (
        "Dutch",
        &["dutch", "niederländisch", "nederlands", "flemish"],
    ),
    ("Polish", &["polish", "polnisch", "polski"]),
    ("Czech", &["czech", "tschechisch", "čeština"]),
    ("Slovak", &["slovak", "slowakisch"]),
    ("Hungarian", &["hungarian", "ungarisch", "magyar"]),
    ("Romanian", &["romanian", "rumänisch"]),
    ("Croatian", &["croatian", "kroatisch"]),
    ("Serbian", &["serbian", "serbisch"]),
    ("Slovenian", &["slovenian", "slowenisch"]),
    ("Bulgarian", &["bulgarian", "bulgarisch"]),
    ("Greek", &["greek", "griechisch"]),
    ("Turkish", &["turkish", "türkisch"]),
    ("Russian", &["russian", "russisch"]),
    ("Ukrainian", &["ukrainian", "ukrainisch"]),
    ("Swedish", &["swedish", "schwedisch"]),
    ("Danish", &["danish", "dänisch"]),
    ("Norwegian", &["norwegian", "norwegisch"]),
    ("Finnish", &["finnish", "finnisch"]),
    ("Arabic", &["arabic", "arabisch"]),
    ("Hebrew", &["hebrew", "hebräisch"]),
    (
        "Chinese",
        &["chinese", "mandarin", "chinesisch", "cantonese"],
    ),
    ("Japanese", &["japanese", "japanisch"]),
    ("Korean", &["korean", "koreanisch"]),
    ("Hindi", &["hindi"]),
];

/// Industries for "industry experience" requirements.
pub static INDUSTRIES: &[(&str, &[&str])] = &[
    (
        "Financial services",
        &[
            "fintech",
            "banking",
            "financial services",
            "finance industry",
            "capital markets",
            "payments",
            "trading",
        ],
    ),
    ("Insurance", &["insurance", "insurtech"]),
    (
        "Healthcare",
        &[
            "healthcare",
            "health care",
            "medtech",
            "medical devices",
            "healthtech",
        ],
    ),
    (
        "Pharma",
        &["pharma", "pharmaceutical", "life sciences", "biotech"],
    ),
    ("Automotive", &["automotive", "mobility"]),
    ("E-commerce", &["e-commerce", "ecommerce", "retail"]),
    (
        "Telecommunications",
        &["telecommunications", "telecom", "telco"],
    ),
    ("Energy", &["energy", "utilities", "renewables"]),
    (
        "Manufacturing",
        &["manufacturing", "industrial", "industry 4.0"],
    ),
    ("Logistics", &["logistics", "supply chain"]),
    ("Public sector", &["public sector", "government", "govtech"]),
    ("Consulting", &["consulting", "professional services"]),
    ("Gaming", &["gaming", "game development"]),
    ("Media", &["media", "publishing", "adtech", "advertising"]),
    (
        "Cybersecurity",
        &["cybersecurity industry", "security industry"],
    ),
];

// ── Tokens ────────────────────────────────────────────────────────────

/// A lowercase word with its byte span in the original text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '+' || c == '#'
}

/// Splits text into lowercase words. `+`, `#` and inner dots belong to words
/// ("c++", "c#", "node.js", ".net"); other punctuation separates them.
pub fn tokens(text: &str) -> Vec<Token> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (start, c) = chars[i];
        let leading_dot = c == '.'
            && chars.get(i + 1).is_some_and(|(_, n)| n.is_alphanumeric())
            && (i == 0 || !chars[i - 1].1.is_alphanumeric());
        if !is_word_char(c) && !leading_dot {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < chars.len() {
            let c = chars[j].1;
            let inner_dot = c == '.'
                && chars[j - 1].1.is_alphanumeric()
                && chars.get(j + 1).is_some_and(|(_, n)| n.is_alphanumeric());
            if is_word_char(c) || inner_dot {
                j += 1;
            } else {
                break;
            }
        }
        let end = chars.get(j).map_or(text.len(), |(b, _)| *b);
        out.push(Token {
            text: text[start..end].to_lowercase(),
            start,
            end,
        });
        i = j;
    }
    out
}

/// The alias key of a phrase: its tokens joined by single spaces.
pub fn key(text: &str) -> String {
    tokens(text)
        .into_iter()
        .map(|t| t.text)
        .collect::<Vec<_>>()
        .join(" ")
}

struct Index {
    /// Alias key → (definition, alias is ambiguous).
    aliases: HashMap<String, (usize, bool)>,
    max_len: usize,
    by_name: HashMap<String, usize>,
}

fn index() -> &'static Index {
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut aliases = HashMap::new();
        let mut by_name = HashMap::new();
        let mut max_len = 1;
        for (i, def) in SKILLS.iter().enumerate() {
            by_name.insert(def.name.to_lowercase(), i);
            let plain = std::iter::once(def.name)
                .filter(|name| !def.ambiguous.iter().any(|a| key(a) == key(name)))
                .chain(def.aliases.iter().copied());
            for alias in plain {
                let k = key(alias);
                max_len = max_len.max(k.split(' ').count());
                aliases.entry(k).or_insert((i, false));
            }
            for alias in def.ambiguous {
                aliases.entry(key(alias)).or_insert((i, true));
            }
        }
        Index {
            aliases,
            max_len,
            by_name,
        }
    })
}

pub fn get(index_of: usize) -> &'static Def {
    &SKILLS[index_of]
}

/// The definition with this canonical name (case-insensitive).
pub fn by_name(name: &str) -> Option<&'static Def> {
    index()
        .by_name
        .get(&name.to_lowercase())
        .map(|&i| &SKILLS[i])
}

/// A dictionary entry found in text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub def: usize,
    /// Byte span in the scanned text.
    pub start: usize,
    pub end: usize,
}

impl Found {
    pub fn def(&self) -> &'static Def {
        &SKILLS[self.def]
    }
}

/// Whether an ambiguous alias sits between list punctuation ("R, SQL",
/// "Python / Go and Rust").
fn list_boundary(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].trim_end().to_lowercase();
    let ok_before = before.is_empty()
        || before.ends_with([',', '/', '(', ';', ':', '•', '-', '*', '|'])
        || before.ends_with(" and")
        || before.ends_with(" or");
    ok_before && list_end(text, end)
}

fn list_end(text: &str, end: usize) -> bool {
    let after = text[end..].trim_start().to_lowercase();
    after.is_empty()
        || after.starts_with([',', '/', ')', ';', '.', '|'])
        || after.starts_with("and ")
        || after.starts_with("or ")
}

/// At the start of a sentence or list item, where any word is capitalized.
fn sentence_start(text: &str, start: usize) -> bool {
    let before = text[..start].trim_end_matches([' ', '\t']);
    before.is_empty() || before.ends_with(['.', '!', '?', ':', '\n', '•', '*', '-'])
}

/// Written like a skill name: exactly the canonical spelling, or all caps.
fn styled(original: &str, name: &str) -> bool {
    original == name || (original.chars().count() >= 2 && !original.chars().any(char::is_lowercase))
}

/// Finds every dictionary entry in prose, longest alias first. Ambiguous
/// aliases ("Go", "React", "Spring") must be written like the skill name;
/// one- or two-letter ones must also stand between list punctuation, and
/// longer ones at the start of a sentence must be followed by it.
pub fn scan(text: &str) -> Vec<Found> {
    let idx = index();
    let toks = tokens(text);
    let mut found = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let mut matched = None;
        for len in (1..=idx.max_len.min(toks.len() - i)).rev() {
            let k = toks[i..i + len]
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            if let Some(&(def, ambiguous)) = idx.aliases.get(&k) {
                let (start, end) = (toks[i].start, toks[i + len - 1].end);
                if ambiguous {
                    let original = &text[start..end];
                    let short = original.chars().count() <= 2;
                    let plausible = styled(original, SKILLS[def].name)
                        && if short {
                            list_boundary(text, start, end)
                        } else {
                            !sentence_start(text, start) || list_end(text, end)
                        };
                    if !plausible {
                        continue;
                    }
                }
                matched = Some((Found { def, start, end }, len));
                break;
            }
        }
        match matched {
            Some((f, len)) => {
                if !found.iter().any(|g: &Found| g.def == f.def) {
                    found.push(f);
                }
                i += len;
            }
            None => i += 1,
        }
    }
    found
}

/// Normalizes one list item ("K8s", "Container orchestration using
/// Kubernetes", "Go"): every dictionary entry it names. In a list, ambiguous
/// aliases count when they are the whole item.
pub fn lookup_item(item: &str) -> Vec<&'static Def> {
    let idx = index();
    if let Some(&(def, _)) = idx.aliases.get(&key(item)) {
        return vec![&SKILLS[def]];
    }
    scan(item).iter().map(Found::def).collect()
}

/// The canonical spoken language for a name, e.g. "Deutsch" → "German".
pub fn language(word: &str) -> Option<&'static str> {
    let w = word.to_lowercase();
    LANGUAGES
        .iter()
        .find(|(_, names)| names.contains(&w.as_str()))
        .map(|(name, _)| *name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(text: &str) -> Vec<&'static str> {
        scan(text).iter().map(|f| f.def().name).collect()
    }

    #[test]
    fn maps_aliases_to_one_canonical_name() {
        for item in [
            "K8s",
            "Kubernetes",
            "Kubernetes clusters",
            "Container orchestration using Kubernetes",
        ] {
            assert_eq!(
                lookup_item(item).iter().map(|d| d.name).collect::<Vec<_>>(),
                ["Kubernetes"],
                "{item}"
            );
        }
        assert_eq!(lookup_item("Postgres")[0].name, "PostgreSQL");
        assert_eq!(lookup_item("PostgreSQL")[0].name, "PostgreSQL");
        assert_eq!(lookup_item("node.js")[0].name, "Node.js");
        assert_eq!(lookup_item("C#")[0].name, "C#");
        assert_eq!(lookup_item("C++")[0].name, "C++");
        assert_eq!(lookup_item("scikit-learn")[0].name, "scikit-learn");
        assert_eq!(lookup_item("Retrieval-Augmented Generation")[0].name, "RAG");
        assert_eq!(lookup_item("CI/CD")[0].name, "CI/CD");
        assert_eq!(lookup_item(".NET Core")[0].name, ".NET");
        // Ambiguous words count when they are the whole list item.
        assert_eq!(lookup_item("Go")[0].name, "Go");
        assert_eq!(lookup_item("react")[0].name, "React");
        assert!(lookup_item("Leadership of a small team")
            .iter()
            .any(|d| d.name == "Leadership"));
    }

    #[test]
    fn scans_prose_without_false_positives() {
        assert_eq!(
            names("Experience with Python, Go and Kubernetes (EKS); Terraform is a plus."),
            ["Python", "Go", "Kubernetes", "Terraform"]
        );
        // Ordinary words stay words.
        assert!(names("You will go beyond and react quickly in spring.").is_empty());
        assert!(names("We expect you to excel at communication").contains(&"Communication"));
        assert!(!names("We expect you to excel at communication").contains(&"Excel"));
        assert_eq!(
            names("Strong React and Spring Boot skills"),
            ["React", "Spring Boot"]
        );
        assert_eq!(names("Languages: R, SQL"), ["R", "SQL"]);
        assert!(names("R&D department").is_empty());
        assert!(names("Express your interest today. Go ahead!").is_empty());
        assert_eq!(names("C-level roles. Languages: C/C++"), ["C", "C++"]);
        assert_eq!(names("Design our REST interfaces"), ["REST APIs"]);
        assert!(names("job security and a great API").is_empty());
        // The longest alias wins.
        assert_eq!(names("React Native apps"), ["React Native"]);
        assert_eq!(names("Apache Spark and PySpark"), ["Apache Spark"]);
    }

    #[test]
    fn tokenizes_skill_names() {
        let t: Vec<String> = tokens("Node.js, C++ & C#; .NET. Done.")
            .into_iter()
            .map(|t| t.text)
            .collect();
        assert_eq!(t, ["node.js", "c++", "c#", ".net", "done"]);
    }

    #[test]
    fn every_alias_is_unique() {
        let mut seen = HashMap::new();
        for def in SKILLS {
            let names = [def.name];
            for alias in names.iter().chain(def.aliases).chain(def.ambiguous) {
                if let Some(other) = seen.insert(key(alias), def.name) {
                    assert_eq!(other, def.name, "alias {alias:?} is used twice");
                }
            }
        }
        assert_eq!(language("Deutsch"), Some("German"));
    }
}
