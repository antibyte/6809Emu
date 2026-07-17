export interface AsmExample {
  id: string;
  labelKey: string;
  source: string;
}

export const ASM_EXAMPLES: AsmExample[] = [
  {
    id: "hello",
    labelKey: "examples.hello",
    source: `; ============================================================
; Hello Loop — counter, memory fill, nested delay
; ============================================================
; Good first program for single-step / run:
;   $0200  16-bit tick counter
;   $0210  16-byte ramp rewritten every frame
; Nested delay keeps the loop readable in the UI.
;
; Watch: D, X  |  Memory $0200  |  Memory $0210

        ORG $0100
START   LDS  #$01FF
        CLRA
        CLRB
        STD  $0200

MAIN    LDD  $0200
        ADDD #1
        STD  $0200

        ; fill $0210..$021F with (tick_lo + index)
        LDX  #$0210
        LDB  $0201
        LDA  #16
FILL    STB  ,X+
        ADDB #1
        SUBA #1
        BNE  FILL

        LBSR DELAY
        BRA  MAIN

DELAY   PSHS A,B
        LDA  #$20
DOUT    LDB  #$FF
DIN     SUBB #1
        BNE  DIN
        SUBA #1
        BNE  DOUT
        PULS A,B,PC

        END
`,
  },
  {
    id: "stack",
    labelKey: "examples.stack",
    source: `; ============================================================
; Stack Demo — nested calls and multi-register frames
; ============================================================
; Call chain: MAIN → LEVEL1 → LEVEL2 → LEAF
; Each level writes a marker; B accumulates args on return.
;
; After run:
;   $0300 = $A1   LEVEL1 marker
;   $0301 = $B2   LEVEL2 marker
;   $0302 = $C3   LEAF marker
;   $0303 = $66   sum $11+$22+$33

        ORG $0100
START   LDS  #$01FF
        CLRA
        CLRB
        STD  $0300
        STD  $0302

        LDA  #$11
        BSR  LEVEL1
        STB  $0303
DONE    BRA  DONE

LEVEL1  PSHS A,B,X,Y,U,CC      ; stack: CC,A,B,X,Y,U (low→high)
        LDA  #$A1
        STA  $0300
        LDA  #$22
        BSR  LEVEL2
        ADDB 1,S                ; + saved entry A ($11) at 1,S
        PULS A,B,X,Y,U,CC,PC

LEVEL2  PSHS A,B
        LDA  #$B2
        STA  $0301
        LDA  #$33
        BSR  LEAF
        ADDB 1,S                ; + saved entry A ($22)
        PULS A,B,PC

LEAF    PSHS A
        LDA  #$C3
        STA  $0302
        LDB  ,S                 ; B = $33
        PULS A,PC

        END
`,
  },
  {
    id: "mul",
    labelKey: "examples.mul",
    source: `; ============================================================
; MUL Demo — product table, running sum, non-zero count
; ============================================================
; 1) Showcase 200 * 3 = 600 ($0258) → $0382
; 2) TABLE[i][j] = i*j for i,j in 1..8   (64 bytes @ $0400)
; 3) SUM of all products                  (16-bit @ $0380)
; 4) NZCNT = non-zero cells               (8-bit  @ $0384)

        ORG $0100
START   LDS  #$01FF
        CLRA
        CLRB
        STD  $0380              ; SUM
        STD  $0382              ; WPROD
        STA  $0384              ; NZCNT

        ; showcase MUL
        LDA  #200
        LDB  #3
        MUL
        STD  $0382

        ; fill table + accumulate sum
        LDX  #$0400
        LDA  #1                 ; i
ROW     LDB  #1                 ; j
COL     PSHS A,B                ; save i,j
        MUL                     ; D = i*j, B = low product
        STB  ,X+                ; TABLE[k] = product
        CLRA
        ADDD $0380
        STD  $0380
        PULS A,B                ; restore i,j
        ADDB #1
        CMPB #9
        BNE  COL
        ADDA #1
        CMPA #9
        BNE  ROW

        ; count non-zero entries
        LDX  #$0400
        CLRA                    ; NZCNT in A
WALK    LDB  ,X+
        BEQ  ZSKIP
        ADDA #1
ZSKIP   CMPX #$0440
        BNE  WALK
        STA  $0384

DONE    BRA  DONE
        END
`,
  },
  {
    id: "interrupt",
    labelKey: "examples.interrupt",
    source: `; ============================================================
; Interrupt Demo — SWI soft interrupt + vector table
; ============================================================
; Main loop bumps TICKS and fires SWI every 16 iterations.
; SWI handler bumps SWICNT and sets FLAG=$5A.
; IRQ vector points at a second handler (FLAG=$A5) so hardware
; IRQs from the UI also work.
;
;   $0200  TICKS   (16-bit)
;   $0202  SWICNT  (16-bit)
;   $0204  IRQCNT  (16-bit)
;   $0206  FLAG    (8-bit)
;
; Vectors @ $FFF8: IRQ, SWI, NMI, RESET

        ORG $0100
START   LDS  #$01FF
        CLRA
        CLRB
        STD  $0200
        STD  $0202
        STD  $0204
        STA  $0206
        ANDCC #$EF              ; enable IRQ (clear I)

MAIN    LDD  $0200
        ADDD #1
        STD  $0200
        ANDB #$0F
        BNE  NOSWI
        SWI
NOSWI   LBSR WORK
        BRA  MAIN

WORK    PSHS A,B
        LDA  #$20
W1      SUBA #1
        BNE  W1
        PULS A,B,PC

        ORG $0300
SWI_H   LDD  $0202
        ADDD #1
        STD  $0202
        LDA  #$5A
        STA  $0206
        RTI

        ORG $0320
IRQ_H   LDD  $0204
        ADDD #1
        STD  $0204
        LDA  #$A5
        STA  $0206
        RTI

        ORG $FFF8
        FDB  $0320              ; IRQ
        FDB  $0300              ; SWI
        FDB  $0100              ; NMI
        FDB  $0100              ; RESET
        END
`,
  },
  {
    id: "hd6309",
    labelKey: "examples.hd6309",
    source: `; ============================================================
; HD6309 Demo — native mode, W/Q, TFM, MULD, DIVD, ADDR
; ============================================================
; Requires CPU variant = HD6309.
;
; 1) LDMD #$01 enters native mode
; 2) Fill SRC[$0600] with $10..$1F
; 3) TFM+ copies SRC → DST[$0700]
; 4) MULD 10*10 → Q @ $0802
; 5) DIVD 100/2 → quot $0806, rem $0808
; 6) LEA walk + ADDR inter-register add

        ORG $0100
START   LDS  #$01FF
        LDMD #$01

        LDX  #$0600
        LDA  #$10
        LDB  #16
FILL    STA  ,X+
        ADDA #1
        SUBB #1
        BNE  FILL

        LDX  #$0600
        LDY  #$0700
        LDW  #16
        TFM+ X+,Y+

        LDA  $0700
        STA  $0800
        LDA  $070F
        STA  $0801

        LDD  #10
        LDW  #10
        MULD
        STQ  $0802

        LDD  #100
        DIVD #2
        STD  $0806
        STW  $0808

        LDX  #$0600
        LEAX 5,X
        STX  $080A
        LDY  #$0700
        LEAY -1,Y
        STY  $080C

        LDD  #1000
        LDW  #234
        ADDR W,D
        STD  $080E

        CLRW
IDLE    BRA  IDLE
        END
`,
  },
  {
    id: "coco2",
    labelKey: "examples.coco2",
    source: `; ============================================================
; CoCo 2 Keyboard I/O — PIA0 matrix scan
; ============================================================
; Machine profile: TRS-80 CoCo 2
;
; PIA0 @ $FF00 (Color BASIC layout):
;   $FF00  row read (inputs, active-low)
;   $FF01  CRA  (bit2: 1=data, 0=DDR)
;   $FF02  column drive (outputs, active-low)
;   $FF03  CRB
;
; Scans 8 columns into MATRIX $0500..$0507.
; LASTKEY $0508 = first non-$FF row byte.
; HIT     $0509 = $FF if any key down this frame.

        ORG $0100
START   LDS  #$03FF
        ORCC #$50               ; mask IRQ while demo runs under BASIC ROMs

        ; data-register access
        LDA  #$04
        STA  $FF01
        STA  $FF03

        ; DDRA = $00 (rows in)
        CLRA
        STA  $FF01
        STA  $FF00
        LDA  #$04
        STA  $FF01

        ; DDRB = $FF (cols out)
        CLRA
        STA  $FF03
        LDA  #$FF
        STA  $FF02
        LDA  #$04
        STA  $FF03

        CLRA
        STA  $0508
        STA  $0509

SCAN    CLRA
        STA  $0509              ; HIT = 0
        LDA  #$FE               ; walking zero for column 0
        STA  $050A
        LDX  #$0500

ROW     LDA  $050A
        STA  $FF02              ; column select
        LDA  $FF00              ; row data
        STA  ,X+
        CMPA #$FF
        BEQ  NOROW
        STA  $0508              ; LASTKEY
        LDA  #$FF
        STA  $0509              ; HIT
NOROW   ; next walking-zero: mask = (mask*2)|1, wrap at $FF
        LDA  $050A
        LDB  #2
        MUL
        TFR  B,A
        ORA  #1
        CMPA #$FF
        BNE  MSKOK
        LDA  #$FE
MSKOK   STA  $050A
        CMPX #$0508
        BNE  ROW
        BRA  SCAN
        END
`,
  },
  {
    id: "coco2video",
    labelKey: "examples.coco2video",
    source: `; ============================================================
; CoCo 2 VDG Text — banner + bouncing sprite
; ============================================================
; Machine: TRS-80 CoCo 2  |  Video panel opens with this example
; Text VRAM: 32×16 cells @ $0400 (ASCII space = $20)
;
; Draws once (clear / border / banner), then only erases/moves
; the '*' so Run and single-step stay responsive.
;
; Vars (below VRAM so assemble does not wipe the screen):
;   $0200 POS   offset into VRAM
;   $0202 OLD   previous sprite offset
;   $0204 DX    horizontal step (+1 / $FF)
;   $0205 DY    vertical step   (+1 / $FF)  — applied as ±32

        ORG $0100
START   LDS  #$03FF

        ; PIA1 CRB → data reg, VDG text mode (GM bits clear)
        LDA  #$04
        STA  $FF23
        CLRA
        STA  $FF22

        ; SAM: force screen base $0400 (bit4 set via $FFC9)
        STA  $FFC8              ; clear SA bit 1
        STA  $FFCA              ; clear SA bit 2
        LDA  #$FF
        STA  $FFC9              ; set SA bit 1 → $0400

        LBSR CLS
        LBSR BORDER
        LBSR BANNER

        LDD  #165               ; row 5, col 5
        STD  $0200              ; POS
        STD  $0202              ; OLD
        LDA  #1
        STA  $0204              ; DX = +1
        STA  $0205              ; DY = +1

; ---- animation: erase old, draw new, advance ----------------
FRAME   ; erase previous '*'
        LDD  $0202
        ADDD #$0400
        TFR  D,X
        LDA  #$20
        STA  ,X

        ; draw '*' at POS
        LDD  $0200
        STD  $0202
        ADDD #$0400
        TFR  D,X
        LDA  #$2A
        STA  ,X

        LBSR DELAY

        ; horizontal: POS += DX
        LDB  $0204
        SEX
        ADDD $0200
        STD  $0200

        ; vertical: POS += DY * 32
        LDA  $0205
        BEQ  BOUNCE
        BMI  GOUP
        LDD  $0200
        ADDD #32
        BRA  VOK
GOUP    LDD  $0200
        SUBD #32
VOK     STD  $0200

; bounce on interior cols 1..30 and rows 4..14
BOUNCE  LDD  $0200
        ANDB #31                ; B = column
        CMPB #1
        BNE  NOTL
        LDA  #1
        STA  $0204              ; DX = +1
NOTL    CMPB #30
        BNE  NOTR
        LDA  #$FF
        STA  $0204              ; DX = -1
NOTR    LDD  $0200
        SUBD #128               ; above row 4?
        BCC  NOTTOP
        LDA  #1
        STA  $0205              ; DY = +1
        LDD  $0200
        ANDB #31
        CLRA
        ADDD #128               ; clamp to row 4, same col
        STD  $0200
NOTTOP  LDD  $0200
        SUBD #448               ; at/below row 14?
        BCS  NOTBOT
        LDA  #$FF
        STA  $0205              ; DY = -1
        LDD  $0200
        ANDB #31
        CLRA
        ADDD #448               ; clamp to row 14, same col
        STD  $0200
NOTBOT  LBRA FRAME

; ---- clear 512 bytes to spaces -------------------------------
CLS     LDX  #$0400
        LDA  #$20
        LDB  #0
C1      STA  ,X+
        SUBB #1
        BNE  C1
        LDB  #0
C2      STA  ,X+
        SUBB #1
        BNE  C2
        RTS

; ---- border --------------------------------------------------
BORDER  LDX  #$0400
        LDA  #$2D
        LDB  #32
TOP     STA  ,X+
        SUBB #1
        BNE  TOP
        LDX  #$05E0
        LDB  #32
BOT     STA  ,X+
        SUBB #1
        BNE  BOT
        LDX  #$0400
        LDA  #$21               ; '!' — VDG has no '|'; $7C is '<'
        LDB  #16
SIDE    STA  ,X
        STA  31,X
        LEAX 32,X
        SUBB #1
        BNE  SIDE
        RTS

; ---- banner (strings live at $0280, below VRAM) --------------
BANNER  LDX  #$0422
        LDY  #$0280
B1      LDA  ,Y+
        BEQ  B1D
        STA  ,X+
        BRA  B1
B1D     LDX  #$0442
        LDY  #$0290
B2      LDA  ,Y+
        BEQ  B2D
        STA  ,X+
        BRA  B2
B2D     LDX  #$0462
        LDY  #$02A0
B3      LDA  ,Y+
        BEQ  B3D
        STA  ,X+
        BRA  B3
B3D     RTS

DELAY   PSHS A,B
        LDA  #$04
D1      LDB  #$80
D2      SUBB #1
        BNE  D2
        SUBA #1
        BNE  D1
        PULS A,B,PC

; ---- string data (must stay < $0400) -------------------------
        ORG $0280
        ; "6809EMU" (VDG has no lowercase — bits 0-5 are the glyph)
        FCB $36,$38,$30,$39,$45,$4D,$55,$00
        ORG $0290
        ; "COCO 2 VDG"
        FCB $43,$4F,$43,$4F,$20,$32,$20,$56,$44,$47,$00
        ORG $02A0
        ; "TEXT MODE DEMO"
        FCB $54,$45,$58,$54,$20,$4D,$4F,$44,$45,$20
        FCB $44,$45,$4D,$4F,$00
        END
`,
  },
  {
    id: "coco2sg4",
    labelKey: "examples.coco2sg4",
    source: `; ============================================================
; CoCo 2 Semigraphics 4 — mode switch + patterned frame
; ============================================================
; Machine profile: CoCo 2  |  open the Video panel
;
; $FF22 bit6 (GM2)=1 selects SG4 (SAM V-mode 0).
; VRAM: 512 bytes @ $0400, two SG4 cells per byte.
;
; Draws background, solid frame, checkerboard, center motif.

        ORG $0100
START   LDS  #$03FF
        LDA  #$04
        STA  $FF23
        LDA  #$40               ; GM2 → SG4
        STA  $FF22

        LBSR FILLBG
        LBSR FRAME
        LBSR CHECKER
        LBSR DIAMOND
IDLE    BRA  IDLE

FILLBG  LDX  #$0400
        LDA  #$11
        LDB  #0
F1      STA  ,X+
        SUBB #1
        BNE  F1
        LDB  #0
F2      STA  ,X+
        SUBB #1
        BNE  F2
        RTS

FRAME   LDX  #$0400
        LDA  #$FF
        LDB  #32
FT      STA  ,X+
        SUBB #1
        BNE  FT
        LDX  #$05E0
        LDB  #32
FB      STA  ,X+
        SUBB #1
        BNE  FB
        LDX  #$0400
        LDB  #16
FS      STA  ,X
        STA  31,X
        LEAX 32,X
        SUBB #1
        BNE  FS
        RTS

; checkerboard: alternate $F0 / $0F via EORA #$FF
CHECKER LDX  #$0421
        LDA  #14
CR      PSHS A
        LDB  #15
        LDA  #$F0
CC      STA  ,X+
        EORA #$FF
        SUBB #1
        BNE  CC
        LEAX 2,X
        PULS A
        SUBA #1
        BNE  CR
        RTS

DIAMOND LDX  #$04EE
        LDA  #$FF
        STA  ,X
        STA  32,X
        STA  -32,X
        LDA  #$F0
        STA  -1,X
        LDA  #$0F
        STA  1,X
        RTS

        END
`,
  },
  {
    id: "aciaecho",
    labelKey: "examples.aciaecho",
    source: `; ============================================================
; ACIA Echo (IRQ) — MC6850 init, banner TX, interrupt RX echo
; ============================================================
; Enable ACIA in the Serial Terminal panel (base $FFA0).
;   $FFA0  data
;   $FFA1  status / control
;
; Emulator control model:
;   write $C0..$FF  master reset
;   CR1 ($02)       RIE (receive IRQ enable)
;   status bit0     RDRF
;   status bit1     TDRE
;
; Flow: reset → enable RIE → poll-TX banner → idle.
; IRQ: echo RX byte when RDRF set; count at $0300.

        ORG $0100
START   LDS  #$01FF
        LDA  #$C0
        STA  $FFA1              ; master reset
        LDA  #$42
        STA  $FFA1              ; RIE on
        ANDCC #$EF              ; clear I

        LDX  #$0180
PUTS    LDA  ,X+
        BEQ  IDLE
WAITTX  LDB  $FFA1
        ANDB #$02
        BEQ  WAITTX
        STA  $FFA0
        BRA  PUTS
IDLE    BRA  IDLE

        ORG $0180
        ; "ACIA Echo ready\r\n"
        FCB $41,$43,$49,$41,$20,$45,$63,$68,$6F,$20
        FCB $72,$65,$61,$64,$79,$0D,$0A,$00

        ORG $0200
IRQ     LDA  $FFA1
        ANDA #$01
        BEQ  NORX
        LDA  $FFA0
W2      LDB  $FFA1
        ANDB #$02
        BEQ  W2
        STA  $FFA0
        LDB  $0300
        ADDB #1
        STB  $0300
NORX    RTI

        ORG $FFF8
        FDB  $0200              ; IRQ
        FDB  $0100              ; SWI
        FDB  $0100              ; NMI
        FDB  $0100              ; RESET
        END
`,
  },
  {
    id: "pia",
    labelKey: "examples.pia",
    source: `; ============================================================
; PIA Demo — MC6821 port I/O with walking-bit output
; ============================================================
; Enable PIA in Setup (base $FF10), open the PIA panel.
;
;   $FF10  ORA/IRA / DDRA   (CRA bit2 selects)
;   $FF11  CRA
;   $FF12  ORB/IRB / DDRB   (CRB bit2 selects)
;   $FF13  CRB
;
; Port A = output (walking LED pattern)
; Port B = input  (toggle bits in the PIA panel)
;
; The output pattern is XORed with the Port B input,
; so flipping input pins changes which output LEDs glow.
; A frame counter at $0200 ticks once per pattern cycle.

        ORG $0100
START   LDS  #$01FF

; ---- configure PIA ------------------------------------------
; Port A: select DDR, set all output, select data reg
        CLRA
        STA  $FF11              ; CRA = $00 → select DDRA
        LDA  #$FF
        STA  $FF10              ; DDRA = $FF (all output)
        LDA  #$04
        STA  $FF11              ; CRA = $04 → select ORA/IRA

; Port B: select DDR, set all input, select data reg
        CLRA
        STA  $FF13              ; CRB = $00 → select DDRB
        CLRA
        STA  $FF12              ; DDRB = $00 (all input)
        LDA  #$04
        STA  $FF13              ; CRB = $04 → select ORB/IRB

; ---- main loop -----------------------------------------------
        LDA  #$01               ; walking bit starts at D0
        CLRB                    ; frame counter

MAIN    ; read Port B input (toggle pins in the PIA panel)
        ; the input value is XORed with our pattern
        EORA $FF12              ; A = pattern ^ input_B

        ; write to Port A → green output LEDs update
        STA  $FF10              ; ORA = pattern

        ; store pattern + frame count for memory view
        STD  $0200              ; $0200 = pattern, $0201 = frame

        ; restore the walking pattern (before XOR) for shifting
        EORA $FF12              ; undo XOR
        INCB                    ; frame++

        ; rotate the walking bit left
        ROLA
        BCC  NOWRAP
        LDA  #$01               ; wrap D7 → D0
NOWRAP

        ; delay so the animation is visible at 1x speed
        LBSR DELAY
        LBRA MAIN

DELAY   PSHS A,B
        LDA  #$10
DOUT    LDB  #$FF
DIN     SUBB #1
        BNE  DIN
        SUBA #1
        BNE  DOUT
        PULS A,B,PC

        END
`,
  },
  {
    id: "aymusic",
    labelKey: "examples.aymusic",
    source: `; ============================================================
; AY-3-8910 Demo — 3-voice melody + envelope sweep
; ============================================================
; Enable AY-3-8910 in Setup (base $FF40), press Run.
;
;   $FF40  address latch  (write register index 0..15)
;   $FF41  data port      (write/read selected register)
;
; Registers:
;   R0/R1   tone period A (12-bit, R1 = coarse & 0x0F)
;   R2/R3   tone period B
;   R4/R5   tone period C
;   R6      noise period (5-bit)
;   R7      mixer: bits 0-5 enable tone/noise (0=on), bit 6/7 port dir
;   R8-R10  amplitude per channel (0-15, bit4 = use envelope)
;   R11/R12 envelope period (16-bit)
;   R13     envelope shape (bit0=Continue, 1=Attack, 2=Alternate, 3=Hold)
;
; Plays an ascending arpeggio on channel A (envelope sweep),
; a low bass drone on channel B, and noise percussion on C.
;
; Memory map:
;   $0200  note index (0..7)
;   $0201  frame counter
;   $0202  note duration counter (frames until next note)

        ORG $0100
START   LDS  #$01FF

; ---- AY init -------------------------------------------------
; R7 mixer (0 = enable, 1 = disable):
;   bit0/1/2 tone A/B/C, bit3/4/5 noise A/B/C
;   tone A+B on, tone C off, noise A+B off, noise C on
;   → %00_011_100 = $1C  (NOT $3B — that muted A/B and left C at period 1 = whistle)
         LDA  #7
         STA  $FF40
         LDA  #$1C
         STA  $FF41

; R6 noise period = 8
         LDA  #6
         STA  $FF40
         LDA  #8
         STA  $FF41

; R8 amplitude A = envelope mode (bit 4 set)
         LDA  #8
         STA  $FF40
         LDA  #$10
         STA  $FF41

; R9 amplitude B = fixed level 12
         LDA  #9
         STA  $FF40
         LDA  #$0C
         STA  $FF41

; R10 amplitude C = fixed level 10
         LDA  #10
         STA  $FF40
         LDA  #$0A
         STA  $FF41

; R11/R12 envelope period = 1000 ($03E8)
         LDA  #11
         STA  $FF40
         LDA  #$E8
         STA  $FF41
         LDA  #12
         STA  $FF40
         LDA  #$03
         STA  $FF41

; R13 envelope shape = $0A (Cont+Alt: decay-first triangle \/\/\/)
         LDA  #13
         STA  $FF40
         LDA  #$0A
         STA  $FF41

; ---- bass drone on B: period = 956 (~65 Hz, C2) --------------
; f = chip_clock / 16 / period  →  1e6 / 16 / 956 ≈ 65.4 Hz
         LDA  #2
         STA  $FF40
         LDA  #$BC
         STA  $FF41
         LDA  #3
         STA  $FF40
         LDA  #$03
         STA  $FF41

; ---- init state ----------------------------------------------
         CLRA
         STA  $0200             ; note index
         STA  $0201             ; frame counter
         LDA  #40
         STA  $0202             ; note duration = 40 frames

; ---- main loop -----------------------------------------------
; Every N frames, advance to next note in the arpeggio table
; and write the new tone period to channel A (R0/R1).
; Then run a short delay and loop.

MAIN     LDA  $0201             ; frame++
         ADDA #1
         STA  $0201
         LDA  $0202             ; duration--
         SUBA #1
         STA  $0202
         BNE  SKIPNOTE          ; not time to change note yet

; time for next note — reset duration, advance index
         LDA  #40
         STA  $0202
         LDA  $0200             ; note index++
         ADDA #1
         STA  $0200
         LDA  $0200
         ANDA #$07              ; wrap 0..7
         STA  $0200

; read 16-bit period from NOTE_TABLE[index] and write to R0/R1
         LDB  $0200
         ASLB                  ; index * 2
         LDX  #$0280
         ABX                   ; X = $0280 + index*2

         LDA  ,X               ; fine byte
         PSHS B
         LDB  #0
         STB  $FF40             ; select R0
         STA  $FF41             ; write fine

         LDA  1,X              ; coarse byte
         LDB  #1
         STB  $FF40             ; select R1
         STA  $FF41             ; write coarse
         PULS B

; retrigger envelope (write R13 again to restart sweep)
         LDA  #13
         STA  $FF40
         LDA  #$0A
         STA  $FF41

SKIPNOTE BSR  DELAY
         BRA  MAIN

; ---- delay (~20 ms at 1 MHz) ---------------------------------
DELAY    PSHS A,B
         LDA  #$20
D1       LDB  #$FF
D2       SUBB #1
         BNE  D2
         SUBA #1
         BNE  D1
         PULS A,B,PC

; ---- note table: 8 x 16-bit tone periods ---------------------
; C major arpeggio: C4 E4 G4 C5 E5 G5 C6 E6
; period = chip_clock / 16 / freq,  chip_clock = 1 MHz
;   C4  262 Hz -> 238  ($00EE)
;   E4  330 Hz -> 189  ($00BD)
;   G4  392 Hz -> 159  ($009F)
;   C5  523 Hz -> 119  ($0077)
;   E5  659 Hz ->  95  ($005F)
;   G5  784 Hz ->  80  ($0050)
;   C6 1047 Hz ->  60  ($003C)
;   E6 1319 Hz ->  47  ($002F)
        ORG $0280
NOTETAB FCB $EE,$00, $BD,$00, $9F,$00, $77,$00
        FCB $5F,$00, $50,$00, $3C,$00, $2F,$00

         END
`,
  },
];
