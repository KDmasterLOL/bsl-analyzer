//! ScheduledJob XML parser

use crate::error::Result;
use crate::scheduled_job::ScheduledJob;

use super::helpers::parse_uuid;
use super::serde_types::ScheduledJobRoot;

/// Parse ScheduledJob XML from Designer format
///
/// # Arguments
///
/// * `xml` - XML content as string
///
/// # Returns
///
/// Parsed `ScheduledJob` structure
///
/// # Example
///
/// ```no_run
/// # use bsl_metadata::xml_parser::parse_scheduled_job_xml;
/// let xml = std::fs::read_to_string("ScheduledJobs/MyJob.xml")?;
/// let job = parse_scheduled_job_xml(&xml)?;
/// # Ok::<(), bsl_metadata::MetadataError>(())
/// ```
pub fn parse_scheduled_job_xml(xml: &str) -> Result<ScheduledJob> {
    let _span = tracing::debug_span!("parse_scheduled_job_xml").entered();

    let root: ScheduledJobRoot = quick_xml::de::from_str(xml)?;
    let uuid = parse_uuid(&root.scheduled_job.uuid, "scheduled job")?;

    let job = ScheduledJob {
        uuid,
        name: root.scheduled_job.properties.name,
        method_name: root.scheduled_job.properties.method_name,
        predefined: root.scheduled_job.properties.predefined.into(),
        use_flag: root.scheduled_job.properties.use_flag.into(),
    };

    tracing::debug!(
        job_name = %job.name(),
        uuid = %job.uuid,
        method_name = %job.method_name(),
        predefined = job.is_predefined(),
        "parsed scheduled job"
    );

    Ok(job)
}
