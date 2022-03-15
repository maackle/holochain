//! The CellConductorApi allows Cells to talk to their Conductor

use std::sync::Arc;

use super::error::ConductorApiError;
use super::error::ConductorApiResult;
use crate::conductor::interface::SignalBroadcaster;
use crate::conductor::ConductorHandle;
use crate::core::ribosome::guest_callback::post_commit::PostCommitArgs;
use crate::core::workflow::ZomeCallResult;
use async_trait::async_trait;
use holo_hash::DnaHash;
use holochain_conductor_api::ZomeCall;
use holochain_keystore::MetaLairClient;
use holochain_state::host_fn_workspace::SourceChainWorkspace;
use holochain_types::prelude::*;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::OwnedPermit;
use tracing::*;

/// The concrete implementation of [CellConductorApiT], which is used to give
/// Cells an API for calling back to their [Conductor].
#[derive(Clone)]
pub struct CellConductorApi {
    conductor_handle: ConductorHandle,
    cell_id: CellId,
}

/// A handle that can only call zome functions to avoid
/// making write lock calls
pub type CellConductorReadHandle = Arc<dyn CellConductorReadHandleT>;

impl CellConductorApi {
    /// Instantiate from a Conductor reference and a CellId to identify which Cell
    /// this API instance is associated with
    pub fn new(conductor_handle: ConductorHandle, cell_id: CellId) -> Self {
        Self {
            conductor_handle,
            cell_id,
        }
    }
}

#[async_trait]
impl CellConductorApiT for CellConductorApi {
    fn cell_id(&self) -> &CellId {
        &self.cell_id
    }

    async fn call_zome(
        &self,
        cell_id: &CellId,
        call: ZomeCall,
    ) -> ConductorApiResult<ZomeCallResult> {
        if *cell_id == call.cell_id {
            self.conductor_handle
                .call_zome(call)
                .await
                .map_err(Into::into)
        } else {
            Err(ConductorApiError::ZomeCallCellMismatch {
                api_cell_id: cell_id.clone(),
                call_cell_id: call.cell_id,
            })
        }
    }

    async fn dpki_request(&self, _method: String, _args: String) -> ConductorApiResult<String> {
        warn!("Using placeholder dpki");
        Ok("TODO".to_string())
    }

    fn keystore(&self) -> &MetaLairClient {
        self.conductor_handle.keystore()
    }

    async fn signal_broadcaster(&self) -> SignalBroadcaster {
        self.conductor_handle.signal_broadcaster().await
    }

    fn get_dna(&self, dna_hash: &DnaHash) -> Option<DnaFile> {
        self.conductor_handle.get_dna_file(dna_hash)
    }

    fn get_this_dna(&self) -> ConductorApiResult<DnaFile> {
        self.conductor_handle
            .get_dna_file(self.cell_id.dna_hash())
            .ok_or_else(|| ConductorApiError::DnaMissing(self.cell_id.dna_hash().clone()))
    }

    fn get_zome(&self, dna_hash: &DnaHash, zome_name: &ZomeName) -> ConductorApiResult<Zome> {
        Ok(self
            .get_dna(dna_hash)
            .ok_or_else(|| ConductorApiError::DnaMissing(dna_hash.clone()))?
            .dna_def()
            .get_zome(zome_name)?)
    }

    fn get_entry_def(&self, key: &EntryDefBufferKey) -> Option<EntryDef> {
        self.conductor_handle.get_entry_def(key)
    }

    fn into_call_zome_handle(self) -> CellConductorReadHandle {
        Arc::new(self)
    }

    async fn post_commit_permit(&self) -> Result<OwnedPermit<PostCommitArgs>, SendError<()>> {
        self.conductor_handle.post_commit_permit().await
    }
}

/// The "internal" Conductor API interface, for a Cell to talk to its calling Conductor.
#[async_trait]
pub trait CellConductorApiT: Clone + Send + Sync + Sized {
    /// Get this cell id
    fn cell_id(&self) -> &CellId;

    /// Invoke a zome function on any cell in this conductor.
    /// A zome call on a different Cell than this one corresponds to a bridged call.
    async fn call_zome(
        &self,
        cell_id: &CellId,
        call: ZomeCall,
    ) -> ConductorApiResult<ZomeCallResult>;

    /// Make a request to the DPKI service running for this Conductor.
    /// TODO: decide on actual signature
    async fn dpki_request(&self, method: String, args: String) -> ConductorApiResult<String>;

    /// Request access to this conductor's keystore
    fn keystore(&self) -> &MetaLairClient;

    /// Access the broadcast Sender which will send a Signal across every
    /// attached app interface
    async fn signal_broadcaster(&self) -> SignalBroadcaster;

    /// Get a [`Dna`](holochain_types::prelude::Dna) from the [`DnaStore`](crate::conductor::dna_store::DnaStore)
    fn get_dna(&self, dna_hash: &DnaHash) -> Option<DnaFile>;

    /// Get the [`Dna`](holochain_types::prelude::Dna) of this cell from the [`DnaStore`](crate::conductor::dna_store::DnaStore)
    fn get_this_dna(&self) -> ConductorApiResult<DnaFile>;

    /// Get a [`Zome`](holochain_types::prelude::Zome) from this cell's Dna
    fn get_zome(&self, dna_hash: &DnaHash, zome_name: &ZomeName) -> ConductorApiResult<Zome>;

    /// Get a [`EntryDef`](holochain_zome_types::EntryDef) from the [`EntryDefBufferKey`](holochain_types::dna::EntryDefBufferKey)
    fn get_entry_def(&self, key: &EntryDefBufferKey) -> Option<EntryDef>;

    /// Turn this into a call zome handle
    fn into_call_zome_handle(self) -> CellConductorReadHandle;

    /// Get an OwnedPermit to the post commit task.
    async fn post_commit_permit(&self) -> Result<OwnedPermit<PostCommitArgs>, SendError<()>>;
}

#[async_trait]
/// A handle that cn only call zome functions to avoid
/// making write lock calls
pub trait CellConductorReadHandleT: Send + Sync {
    /// Get this cell id
    fn cell_id(&self) -> &CellId;

    /// Invoke a zome function on a Cell
    async fn call_zome(
        &self,
        call: ZomeCall,
        workspace_lock: SourceChainWorkspace,
    ) -> ConductorApiResult<ZomeCallResult>;

    /// Get a zome from this cell's Dna
    fn get_zome(&self, dna_hash: &DnaHash, zome_name: &ZomeName) -> ConductorApiResult<Zome>;
}

#[async_trait]
impl CellConductorReadHandleT for CellConductorApi {
    fn cell_id(&self) -> &CellId {
        &self.cell_id
    }

    async fn call_zome(
        &self,
        call: ZomeCall,
        workspace_lock: SourceChainWorkspace,
    ) -> ConductorApiResult<ZomeCallResult> {
        if self.cell_id == call.cell_id {
            self.conductor_handle
                .call_zome_with_workspace(call, workspace_lock)
                .await
        } else {
            self.conductor_handle.call_zome(call).await
        }
    }

    fn get_zome(&self, dna_hash: &DnaHash, zome_name: &ZomeName) -> ConductorApiResult<Zome> {
        CellConductorApiT::get_zome(self, dna_hash, zome_name)
    }
}
