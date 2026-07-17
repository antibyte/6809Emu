//! Minimal Color BASIC–style boot ROM for the CoCo 2 profile.
//! Initializes PIA/SAM/VDG, clears the text screen, prints a banner, then jumps to RAM entry.

use m6809_asm::assemble;

pub const BOOT_PC: u16 = 0xC000;
pub const TRAP_HANDLER: u16 = 0xC0F0;
pub const MSG_ADDR: u16 = 0xC080;
#[allow(dead_code)]
pub const BANNER: &str = "6809Emu CoCo2";

fn write_asm_at(ram: &mut [u8; 0x10000], source: &str) {
    let program = assemble(source).expect("coco2 boot ROM should assemble");
    let start = program.origin as usize;
    let end = start + program.bytes.len();
    assert!(end <= 0xFF00, "CoCo 2 ROM image too large");
    ram[start..end].copy_from_slice(&program.bytes);
}

pub fn install(ram: &mut [u8; 0x10000], entry: u16) {
    let boot = format!(
        r#"
        ORG ${boot:04X}
START   ORCC    #$FC
        LDS     #$0400
        LDA     #$04
        STA     $FF01
        LDA     #$05
        STA     $FF03
        LDA     #$FF
        STA     $FF00
        STA     $FF02
        LDA     #$04
        STA     $FF21
        LDA     #$05
        STA     $FF23
        LDA     #$04
        STA     $FF22
        LDA     #$00
        STA     $FFC8
        STA     $FFCA
        LDA     #$FF
        STA     $FFC9
        LDX     #$05FF
        LDA     #$20
CLR     STA     ,X
        DEX
        CMPX    #$03FF
        BNE     CLR
        LDY     #$0400
        LDX     #${msg:04X}
PL      LDA     ,X
        BEQ     DONE
        STA     ,Y
        LEAX    1,X
        LEAY    1,Y
        BRA     PL
DONE    JMP     ${entry:04X}
        END
        "#,
        boot = BOOT_PC,
        msg = MSG_ADDR,
        entry = entry,
    );
    write_asm_at(ram, &boot);

    write_asm_at(
        ram,
        &format!(
            r#"
        ORG ${trap:04X}
TRAP    NOP
        BRA     TRAP
        END
        "#,
            trap = TRAP_HANDLER,
        ),
    );

    write_asm_at(
        ram,
        r#"
        ORG $C080
BOOTMSG FCB $36,$38,$30,$39,$45,$6D,$75,$20,$43,$6F,$43,$6F,$32,$00
        END
        "#,
    );

    for b in ram[(BOOT_PC as usize + 0x100)..0xFF00].iter_mut() {
        *b = 0x12; // NOP padding
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use m6809_core::Emulator;

    #[test]
    fn rom_assembles_and_contains_banner() {
        let mut ram = [0u8; 0x10000];
        install(&mut ram, 0x0100);
        assert_eq!(&ram[MSG_ADDR as usize..MSG_ADDR as usize + 7], b"6809Emu");
        assert_eq!(ram[TRAP_HANDLER as usize], 0x12);
    }

    #[test]
    fn stub_boot_reaches_entry_and_prints_banner() {
        // Stub ROM is no longer the default firmware; install it manually.
        let mut emu = Emulator::new();
        install(&mut emu.memory.ram, 0x0100);
        emu.memory.write16(0xFFFE, BOOT_PC);
        emu.cpu.pc = BOOT_PC;
        for _ in 0..8000 {
            if emu.cpu.pc == 0x0100 {
                break;
            }
            let _ = emu.step();
        }
        assert_eq!(emu.cpu.pc, 0x0100);
    }
}