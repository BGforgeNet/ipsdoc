// The part of GemRB's stream interface its ACM unpacker uses, over a memory buffer.
#ifndef IPSDOC_GEMRB_SHIM_DATASTREAM_H
#define IPSDOC_GEMRB_SHIM_DATASTREAM_H

#include <cstddef>
// The unpacker gets malloc and free through this header, as through the real one.
#include <cstdlib>
#include <cstring>

namespace GemRB {

class DataStream {
public:
    DataStream(const unsigned char* data, size_t len)
        : data(data), len(len)
    {
    }

    unsigned long Remains() const { return static_cast<unsigned long>(len - pos); }

    long Read(void* dest, size_t size)
    {
        if (size > len - pos) {
            size = len - pos;
        }
        memcpy(dest, data + pos, size);
        pos += size;
        return static_cast<long>(size);
    }

private:
    const unsigned char* data;
    size_t len;
    size_t pos = 0;
};

} // namespace GemRB

#endif
