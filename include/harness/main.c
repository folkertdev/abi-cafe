// This is the primary file for the abi-cafe standalone binary/project mode,
// for tests where both sides are c.
//
// It's the equivalent of main.rs; see that file for the rationale.
//
// As such this is incompatible with `WriteImpl::HarnessCallback`.

extern void do_test(void);

int main(void) {
    do_test();
    return 0;
}
