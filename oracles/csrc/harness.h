/* Shared by the decoder harnesses: `<harness> in.acm out.raw` decodes a bare ACM to 16-bit little-endian samples,
 * at most the header's sample count, and exits 1 if the decoder rejects the stream. `-` is stdin or stdout. */
#ifndef IPSDOC_HARNESS_H
#define IPSDOC_HARNESS_H

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Decodes `len` bytes at `data`, writing up to `cap` samples to `out`; returns how many, or a negative error. */
#ifdef __cplusplus
extern "C"
#endif
long harness_decode(const unsigned char *data, size_t len, short *out, size_t cap);

static int harness_main(int argc, char **argv)
{
    if (argc != 3) {
        fprintf(stderr, "usage: %s in.acm out.raw\n", argv[0]);
        return 2;
    }
    int std_in = strcmp(argv[1], "-") == 0, std_out = strcmp(argv[2], "-") == 0;
    FILE *in = std_in ? stdin : fopen(argv[1], "rb");
    if (!in) {
        perror(argv[1]);
        return 2;
    }
    size_t len = 0, size = 1 << 16;
    unsigned char *data = (unsigned char *)malloc(size);
    size_t n;
    while ((n = fread(data + len, 1, size - len, in)) > 0) {
        len += n;
        if (len == size)
            data = (unsigned char *)realloc(data, size *= 2);
    }
    if (ferror(in)) {
        perror(argv[1]);
        return 2;
    }
    if (!std_in)
        fclose(in);
    if (len < 8) {
        fprintf(stderr, "%s: %zu bytes, shorter than an ACM header\n", argv[1], len);
        return 1;
    }
    size_t cap = data[4] | data[5] << 8 | data[6] << 16 | (size_t)data[7] << 24;
    short *out = (short *)malloc(cap > 0 ? cap * sizeof(short) : 1);
    long got = harness_decode(data, len, out, cap);
    if (got < 0) {
        fprintf(stderr, "%s: rejected, error %ld\n", argv[1], got);
        return 1;
    }
    /* Samples are written in host order: every supported host is little-endian. */
    FILE *o = std_out ? stdout : fopen(argv[2], "wb");
    if (!o || fwrite(out, sizeof(short), (size_t)got, o) != (size_t)got || fclose(o) != 0) {
        perror(argv[2]);
        return 2;
    }
    return 0;
}

#endif
