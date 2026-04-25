.global _start

_start:
    MOV r0, #5       /* Put 5 in register 0 */
    MOV r1, #10      /* Put 10 in register 1 */
    ADD r2, r0, r1   /* Add them: r2 = 15 */
    
stop:
    B stop           /* Infinite loop to halt execution */