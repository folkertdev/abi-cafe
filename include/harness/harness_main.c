// This is the primary file for the abi-cafe harness main that c-only tests are
// compiled into.
//
// It's the equivalent of harness_main.rs, and exists so that a test where both
// sides are c doesn't need a rust stdlib for the target (or rustc at all) just
// to link the final binary.
//
// This will be statically linked with two other static libraries: the caller
// and the callee. The caller is expected to define the function `do_test`, and
// call a bunch of functions defined by the callee. The values both sides see
// are reported on stdout as json lines, which the harness parses back.
//
// This instrumentation is only used in the default mode of `WriteImpl::HarnessCallback`.
// Otherwise the caller/callee may use things like asserts/prints.

#include <inttypes.h>
#include <stdio.h>

// From the test's perspective the WriteBuffers are totally opaque, so ours is
// just the name we report the values under.
#define WriteBuffer void*

typedef void (*SetFuncCallback)(WriteBuffer, uint32_t);
typedef void (*WriteValCallback)(WriteBuffer, uint32_t, char*, uint32_t);

WriteBuffer CALLER_VALS = NULL;
WriteBuffer CALLEE_VALS = NULL;
SetFuncCallback SET_FUNC = NULL;
WriteValCallback WRITE_VAL = NULL;

static void set_func(WriteBuffer vals, uint32_t func) {
    printf("{ \"info\": \"func\", \"id\": \"%s\", \"func\": %" PRIu32 " }\n", (const char*)vals, func);
}

static void write_val(WriteBuffer vals, uint32_t val_idx, char* input, uint32_t size) {
    printf("{ \"info\": \"val\", \"id\": \"%s\", \"val\": %" PRIu32 ", \"bytes\": [", (const char*)vals, val_idx);
    for (uint32_t i = 0; i < size; i++) {
        printf(i == 0 ? "%u" : ", %u", (unsigned int)(unsigned char)input[i]);
    }
    printf("] }\n");
}

extern void do_test(void);

int main(void) {
    CALLER_VALS = (WriteBuffer)"caller";
    CALLEE_VALS = (WriteBuffer)"callee";
    SET_FUNC = set_func;
    WRITE_VAL = write_val;

    do_test();
    printf("{ \"info\": \"done\" }\n");
    return 0;
}
