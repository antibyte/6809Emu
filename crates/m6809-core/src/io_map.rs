use serde::{Deserialize, Serialize};

/// Result of a memory write through an optional I/O mapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoWriteResult {
    /// Write should proceed to backing RAM.
    PassThrough,
    /// Address was handled by I/O hardware; do not write RAM.
    Consumed,
    /// Write was ignored (e.g. ROM region).
    Ignored,
}

/// Human-readable I/O register for the debugger UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoRegisterView {
    pub address: u16,
    pub name: String,
    pub value: u8,
}

/// Optional machine-specific memory mapping and I/O simulation.
pub trait MemoryIo: Send + std::fmt::Debug {
    fn kind_id(&self) -> &str;
    fn read(&self, addr: u16, ram: &[u8; 0x10000]) -> Option<u8>;
    fn write(&mut self, addr: u16, value: u8, ram: &mut [u8; 0x10000]) -> IoWriteResult;
    fn clone_box(&self) -> Box<dyn MemoryIo>;
    fn snapshot(&self) -> serde_json::Value;
    fn restore(&mut self, snapshot: &serde_json::Value);
    fn io_registers(&self) -> Vec<IoRegisterView>;
    fn tick(&mut self, _cycles: u32) {}
    fn poll_irq(&mut self) -> bool {
        false
    }
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}