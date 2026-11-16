use crate::{
    AppResult,
    domain::models::{
        admin_user::{AdminUser, AdminUserPassword},
        auth::LoginRequest,
        security_notice::{ListSecNoticeRequest, ListSecNoticeResponseData},
        sync_data_task::{CreateSyncDataTaskRequest, SyncDataTask},
        vuln_information::{
            GetVulnInformationRequest, ListVulnInformationRequest, ListVulnInformationResponseData,
            VulnInformation,
        },
    },
};
use std::future::Future;

pub trait VulnService: Clone + Send + Sync + 'static {
    fn login(&self, req: &LoginRequest) -> impl Future<Output = AppResult<AdminUser>> + Send;
    fn create_sync_data_task(
        &self,
        req: CreateSyncDataTaskRequest,
    ) -> impl Future<Output = AppResult<i64>> + Send;
    fn get_sync_data_task(&self) -> impl Future<Output = AppResult<Option<SyncDataTask>>> + Send;

    fn list_vulnfusion_information(
        &self,
        req: ListVulnInformationRequest,
    ) -> impl Future<Output = AppResult<ListVulnInformationResponseData>> + Send;

    fn get_vuln_information(
        &self,
        req: GetVulnInformationRequest,
    ) -> impl Future<Output = AppResult<Option<VulnInformation>>> + Send;

    fn change_admin_password(
        &self,
        old_password: AdminUserPassword,
        new_password: AdminUserPassword,
    ) -> impl Future<Output = AppResult<()>> + Send;

    fn list_sec_notice(
        &self,
        req: ListSecNoticeRequest,
    ) -> impl Future<Output = AppResult<ListSecNoticeResponseData>> + Send;
}

pub trait VulnRepository: Clone + Send + Sync + 'static {
    fn login(&self, req: &LoginRequest) -> impl Future<Output = AppResult<AdminUser>> + Send;
    fn create_sync_data_task(
        &self,
        req: CreateSyncDataTaskRequest,
    ) -> impl Future<Output = AppResult<i64>> + Send;

    fn get_sync_data_task(&self) -> impl Future<Output = AppResult<Option<SyncDataTask>>> + Send;

    fn list_vuln_information(
        &self,
        req: ListVulnInformationRequest,
    ) -> impl Future<Output = AppResult<ListVulnInformationResponseData>> + Send;

    fn get_vuln_information(
        &self,
        req: GetVulnInformationRequest,
    ) -> impl Future<Output = AppResult<Option<VulnInformation>>> + Send;

    fn change_admin_password(
        &self,
        old_password: AdminUserPassword,
        new_password: AdminUserPassword,
    ) -> impl Future<Output = AppResult<()>> + Send;

    fn list_sec_notice(
        &self,
        req: ListSecNoticeRequest,
    ) -> impl Future<Output = AppResult<ListSecNoticeResponseData>> + Send;
}
