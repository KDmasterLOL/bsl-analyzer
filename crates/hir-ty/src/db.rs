use bsl_types::kind::TypeId;
use hir_def::{ConfigsDatabase, DefWithBodyId, ExprId, MethodIdInput};
use std::sync::Arc;
use vfs::FileId;

use crate::infer::{
    BodyInferenceResult, InferenceDiagnostic, InferenceResult, ModuleCodeInferenceResult,
};
use crate::narrow::NarrowState;
use crate::proc_signature::ProcSignature;

#[salsa::db]
pub trait HirDatabase: ConfigsDatabase + bsl_types::intern::TypeKernelDb {
    fn infer(&self, file_id: FileId) -> Arc<InferenceResult>;

    fn type_of_expr(&self, file_id: FileId, owner: DefWithBodyId, expr: ExprId) -> TypeId;

    fn narrow(
        &self,
        file_id: FileId,
        owner: DefWithBodyId,
    ) -> Option<Arc<dataflow::DataflowResult<NarrowState>>>;

    fn arg_diagnostics(&self, file_id: FileId) -> Arc<Vec<(DefWithBodyId, InferenceDiagnostic)>>;

    fn type_narrowing_enabled(&self) -> bool;

    /// Project-level execution-environment settings for API-availability
    /// checks (which client environments the configuration targets).
    fn env_options(&self) -> hir_def::execution_env::EnvOptions;

    fn proc_signature(&self, method_input: MethodIdInput<'_>) -> Arc<ProcSignature>;

    fn infer_method(&self, method: MethodIdInput<'_>) -> Arc<BodyInferenceResult>;

    fn infer_module_code(&self, file_id: FileId) -> Arc<ModuleCodeInferenceResult>;

    fn module_reaching_definitions(
        &self,
        file_id: FileId,
    ) -> Arc<dataflow::reaching_defs::ModuleReachingDefs>;
}
