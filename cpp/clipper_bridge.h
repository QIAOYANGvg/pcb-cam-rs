#pragma once

#include <cstddef>
#include <cstdint>

extern "C"
{
    struct GERBER_CLIPPER_POINT64
    {
        std::int64_t x;
        std::int64_t y;
    };

    void* gerber_clipper_execute( int operation,
                                  const GERBER_CLIPPER_POINT64* subjectPoints,
                                  const std::size_t* subjectOffsets,
                                  std::size_t subjectPathCount,
                                  const GERBER_CLIPPER_POINT64* clipPoints,
                                  const std::size_t* clipOffsets,
                                  std::size_t clipPathCount );

    void gerber_clipper_tree_delete( void* tree );

    std::size_t gerber_clipper_node_child_count( const void* node );
    const void* gerber_clipper_node_child( const void* node, std::size_t index );
    std::size_t gerber_clipper_node_point_count( const void* node );
    bool gerber_clipper_node_point( const void* node, std::size_t index,
                                    GERBER_CLIPPER_POINT64* point );
}
