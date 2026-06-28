export interface AsmExample {
  id: string;
  labelKey: string;
  source: string;
}

export const ASM_EXAMPLES: AsmExample[] = [
  {
    id: "hello",
    labelKey: "examples.hello",
    source: `        ORG $0100
start   LDA  #$42
        NOP
        BRA  start
        END`,
  },
  {
    id: "stack",
    labelKey: "examples.stack",
    source: `        ORG $0100
        LDS  #$01FF
        LDA  #$41
        PSHS A
        PULS B
        NOP
        END`,
  },
  {
    id: "mul",
    labelKey: "examples.mul",
    source: `        ORG $0100
        LDA  #$0A
        LDB  #$05
        MUL
        NOP
        END`,
  },
  {
    id: "interrupt",
    labelKey: "examples.interrupt",
    source: `        ORG $0100
        ORCC #$3F
        NOP
loop    BRA  loop
        END`,
  },
  {
    id: "hd6309",
    labelKey: "examples.hd6309",
    source: `        ORG $0100
        LDMD #$01
        LDW  #$000A
        MULD
        LEAX 5,X
        LDX  #$600
        LDY  #$700
        LDW  #$0004
        TFM+ X+,Y+
        DIVD #$0002
        NOP
        END`,
  },
  {
    id: "coco2",
    labelKey: "examples.coco2",
    source: `        ORG $0100
        LDS  #$03FF
        LDA  #$FE
        STA  $FF00
        LDA  $FF02
        STA  $0100
        NOP
        END`,
  },
  {
    id: "coco2video",
    labelKey: "examples.coco2video",
    source: `        ORG $0100
        LDX  #$0400
        LDA  #$48
        STA  ,X
        LDA  #$49
        STA  1,X
        NOP
        END`,
  },
  {
    id: "coco2sg4",
    labelKey: "examples.coco2sg4",
    source: `        ORG $0100
        LDA  #$40
        STA  $FF22
        LDX  #$0400
        LDA  #$FF
        STA  ,X
        NOP
        END`,
  },
  {
    id: "aciaecho",
    labelKey: "examples.aciaecho",
    source: `        ORG $0100
        ANDCC #$EF
        LDA  #$03
        STA  $FFA1
        LDA  #$42
        STA  $FFA1
idle    BRA  idle
        ORG $0200
irq     LDA  $FFA1
        ANDA #$01
        BEQ  no_rx
        LDA  $FFA0
        STA  $FFA0
no_rx   RTI
        ORG $FFF8
        FDB  $0200
        END`,
  },
];