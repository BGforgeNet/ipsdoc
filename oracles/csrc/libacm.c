/* Decodes one bare ACM with libacm, keeping the header's channel count. */
#include <string.h>

#include "harness.h"
#include "libacm.h"

struct input {
    const unsigned char *data;
    size_t len;
    size_t pos;
};

static int read_input(void *ptr, int size, int n, void *datasrc)
{
    struct input *in = datasrc;
    size_t want = (size_t)size * (size_t)n;
    size_t left = in->len - in->pos;
    if (want > left)
        want = left - left % (size_t)size;
    memcpy(ptr, in->data + in->pos, want);
    in->pos += want;
    return (int)(want / (size_t)size);
}

long harness_decode(const unsigned char *data, size_t len, short *out, size_t cap)
{
    struct input in = { data, len, 0 };
    acm_io_callbacks io = { read_input, NULL, NULL, NULL };
    ACMStream *acm;
    int err = acm_open_decoder(&acm, &in, io, 0);
    if (err < 0)
        return err;
    int got = acm_read_loop(acm, out, (unsigned)(cap * 2), 0, 2, 1);
    acm_close(acm);
    return got < 0 ? got : got / 2;
}

int main(int argc, char **argv)
{
    return harness_main(argc, argv);
}
