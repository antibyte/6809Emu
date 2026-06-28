//! Boot ROM for the Dragon 32 profile (PIA @ $FF00, SAM @ $FF60).

use m6809_asm::assemble;

pub const BOOT_PC: u16 = 0xC000;
pub const TRAP_HANDLER: u16 = 0xC0F0;
pub const MSG_ADDR: u16 = 0xC080;
#[allow(dead_code)]
pub const BANNER: &str = "6809Emu Dragon";

fn write_asm_at(ram: &mut [u8; 0x10000], source: &str) {
    let program = assemble(source).expect("dragon boot ROM should assemble");
    let start = program.origin as usize;
    let end = start + program.bytes.len();
    assert!(end <= 0xFF00, "Dragon ROM image too large");
    ram[start..end].copy_from_slice(&program.bytes);
}

pub fn install(ram: &mut [u8; 0x10000], entry: u16) {
    let boot = format!(
        r#"
        ORG ${boot:04X}
START   ORCC    #$FC
        LDS     #$0400
        LDA     #$04
        STA     $FF02
        LDA     #$05
        STA     $FF03
        LDA     #$FF
        STA     $FF00
        LDA     #$04
        STA     $FF01
        LDX     #$0DFF
        LDA     #$20
CLR     STA     ,X
        DEX
        CMPX    #$0BFF
        BNE     CLR
        LDY     #$0C00
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
BOOTMSG FCB $36,$38,$30,$39,$45,$6D,$75,$20,$44,$72,$61,$67,$6F,$6E,$00
        END
        "#,
    );

    for b in ram[(BOOT_PC as usize + 0x100)..0xFF00].iter_mut() {
        *b = 0x12;
    }
}