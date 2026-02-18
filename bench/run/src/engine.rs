use bytes::Bytes;
use corevm_engine::{AccumulateEngine, AccumulateOps, OuterVmSimulator, Simulator};
use corevm_host::{
    fs, Arg, CoreVmInstruction, CoreVmOutput, CoreVmPayload, ExecEnv, OutputBuffers, PageAddr,
    PageSegmentOps, StorageKey, VmState,
};
use jam_codec::Encode;
use jam_types::{
    max_accumulate_gas, AccumulateItem, Memo, Segment, SignedGas, TransferRecord, VecMap,
    WorkItemRecord, WorkOutput,
};
use std::{borrow::Cow, convert::Infallible, path::Path};

type LocalOuterVmSimulator =
    OuterVmSimulator<VecMap<PageAddr, Segment>, VecMap<fs::BlockRef, Bytes>>;

struct VecMapStorage(VecMap<StorageKey, Vec<u8>>);

impl AccumulateOps for VecMapStorage {
    type Error = Infallible;

    fn get(&self, key: &StorageKey) -> Option<Cow<'_, [u8]>> {
        self.0.get(key).map(|value| Cow::Borrowed(value.as_ref()))
    }

    fn set(&mut self, key: StorageKey, value: Cow<'_, [u8]>) -> Result<(), Self::Error> {
        self.0.insert(key, value.into_owned());
        Ok(())
    }

    fn remove(&mut self, key: &StorageKey) {
        self.0.remove(key);
    }
}

pub struct LocalEngine {
    step: usize,
    outer_vm: Option<LocalOuterVmSimulator>,
    storage: VecMapStorage,
    payload: CoreVmPayload,
    output: OutputBuffers,
}

impl LocalEngine {
    pub fn new(
        gas: SignedGas,
        program: &[u8],
        root_dir: Option<&Path>,
        exe_args: Vec<Arg>,
        exe_env: Vec<Arg>,
        max_exports: u16,
    ) -> anyhow::Result<Self> {
        let (exec_ref, fs, storage) = reset(gas, program, root_dir, exe_args, exe_env)?;
        let outer_vm = LocalOuterVmSimulator::new(None, fs, max_exports, 0, Default::default())?;
        let payload = CoreVmPayload {
            gas,
            vm_state: VmState::initial(),
            exec_ref,
        };
        Ok(Self {
            step: 0,
            outer_vm: Some(outer_vm),
            storage,
            payload,
            output: Default::default(),
        })
    }

    pub fn step(&mut self) -> anyhow::Result<(CoreVmOutput, LocalOuterVmSimulator)> {
        log::trace!("Step {}", self.step);
        self.output.clear();
        let engine = Simulator::new(
            self.payload.clone(),
            self.outer_vm.take().expect("Finalize was not called"),
        )?;
        let (work_output, outer_vm) = engine.run()?;
        let mut acc_engine = AccumulateEngine::new(&mut self.storage);
        let results = acc_engine.run(&[AccumulateItem::WorkItem(WorkItemRecord {
            package: Default::default(),
            exports_root: Default::default(),
            authorizer_hash: Default::default(),
            auth_output: Default::default(),
            payload: Default::default(),
            gas_limit: max_accumulate_gas(),
            result: Ok(WorkOutput(work_output.encode())),
        })]);
        for result in results {
            result?;
        }
        Ok((work_output, outer_vm))
    }

    /// Advance simulator's state.
    ///
    /// Returns the work payload for the next run.
    ///
    /// - Collects output streams from the program.
    /// - Updates VM state.
    /// - Moves all exported pages to the list of imports.
    pub fn finalize_step(
        &mut self,
        gas: SignedGas,
        work_output: &CoreVmOutput,
        mut outer_vm: LocalOuterVmSimulator,
    ) -> anyhow::Result<()> {
        // Append new data to output streams.
        let mut exports = std::mem::take(outer_vm.exports_mut());
        self.output = OutputBuffers::from_segments(
            &exports[work_output.vm_output.num_memory_pages as usize..],
            &work_output.vm_output.stream_len,
        )?;
        // Generate the payload for the next iteration.
        let payload = CoreVmPayload {
            // Give more gas to the inner VM.
            gas,
            vm_state: work_output.vm_state.clone(),
            exec_ref: work_output.exec_ref,
        };
        // Update memory pages.
        let mut imports = std::mem::take(outer_vm.imports_mut());
        for segment in exports.drain(..work_output.vm_output.num_memory_pages as usize) {
            imports.insert(PageAddr(segment.page_address()), segment);
        }
        *outer_vm.imports_mut() = imports;
        self.payload = payload;
        self.outer_vm = Some(outer_vm);
        self.step += 1;
        Ok(())
    }
}

fn reset(
    gas: SignedGas,
    program: &[u8],
    root_dir: Option<&Path>,
    args: Vec<Arg>,
    env: Vec<Arg>,
) -> anyhow::Result<(fs::BlockRef, VecMap<fs::BlockRef, Bytes>, VecMapStorage)> {
    let mut fs = VecMap::new();
    let program = fs::copy_file_in(
        &mut std::io::Cursor::new(program),
        BOOTSTRAP_SERVICE_ID,
        &mut fs,
        fs::MAX_BLOCK_SIZE,
    )?;
    let root_dir = match root_dir {
        Some(root_dir) => {
            let dir_reader = fs::StdDirReader::new(root_dir.into())?;
            fs::copy_dir_in(
                dir_reader,
                BOOTSTRAP_SERVICE_ID,
                &mut fs,
                fs::MAX_BLOCK_SIZE,
            )?
        }
        None => fs::BlockRef {
            service_id: BOOTSTRAP_SERVICE_ID,
            hash: Default::default(),
        },
    };
    let exec = ExecEnv {
        program,
        root_dir,
        args,
        env,
    };
    let exec_encoded = exec.encode();
    let exec_ref = fs::copy_file_in(
        &mut std::io::Cursor::new(&exec_encoded[..]),
        BOOTSTRAP_SERVICE_ID,
        &mut fs,
        fs::MAX_BLOCK_SIZE,
    )?;
    let mut memo = Memo::default();
    CoreVmInstruction::Reset { gas, exec_ref }.encode_to(&mut &mut memo[..]);
    let mut engine = AccumulateEngine::new(VecMapStorage(VecMap::new()));
    let results = engine.run(&[AccumulateItem::Transfer(TransferRecord {
        source: BOOTSTRAP_SERVICE_ID,
        destination: COREVM_SERVICE_ID,
        amount: 10_000_000_000,
        memo,
        gas_limit: 1_000_000,
    })]);
    for result in results {
        result?;
    }
    let storage = engine.into_inner();
    Ok((exec_ref, fs, storage))
}

const BOOTSTRAP_SERVICE_ID: u32 = 0;
const COREVM_SERVICE_ID: u32 = 1;
