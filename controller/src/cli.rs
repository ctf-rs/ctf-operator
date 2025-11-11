use clap::Parser;

#[derive(Debug, Parser)]
#[clap(about = "CTF Operator")]
pub struct Args {
    #[arg(long, env = "KUBERT_EXAMPLE_LOG", default_value = "lease=info,warn")]
    pub log_level: kubert::LogFilter,

    /// The logging format
    #[arg(long, default_value = "plain")]
    pub log_format: kubert::LogFormat,

    #[clap(flatten)]
    pub client: kubert::ClientArgs,

    #[clap(long, default_value = "ctf")]
    pub ctf_deployment_name: String,

    #[clap(long, default_value = "ctf")]
    pub control_plane_namespace: String,

    #[clap(long, default_value = "5000")]
    pub patch_timeout_ms: u64,
}
