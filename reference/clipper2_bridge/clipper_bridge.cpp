#include "clipper_bridge.h"

#include <clipper2/clipper.h>

#include <memory>
#include <utility>

namespace
{
using namespace Clipper2Lib;

Paths64 loadPaths( const GERBER_CLIPPER_POINT64* points, const std::size_t* offsets,
                   std::size_t pathCount )
{
    Paths64 paths;
    paths.reserve( pathCount );

    for( std::size_t pathIndex = 0; pathIndex < pathCount; ++pathIndex )
    {
        const std::size_t begin = offsets[pathIndex];
        const std::size_t end = offsets[pathIndex + 1];

        if( end - begin < 3 )
            continue;

        Path64 path;
        path.reserve( end - begin );

        for( std::size_t pointIndex = begin; pointIndex < end; ++pointIndex )
            path.emplace_back( points[pointIndex].x, points[pointIndex].y );

        paths.emplace_back( std::move( path ) );
    }

    return paths;
}
}

extern "C" void* gerber_clipper_execute( int operation,
                                          const GERBER_CLIPPER_POINT64* subjectPoints,
                                          const std::size_t* subjectOffsets,
                                          std::size_t subjectPathCount,
                                          const GERBER_CLIPPER_POINT64* clipPoints,
                                          const std::size_t* clipOffsets,
                                          std::size_t clipPathCount )
{
    try
    {
        Clipper64 clipper;
        auto solution = std::make_unique<PolyTree64>();
        Paths64 subjects = loadPaths( subjectPoints, subjectOffsets, subjectPathCount );
        Paths64 clips = loadPaths( clipPoints, clipOffsets, clipPathCount );

        clipper.AddSubject( subjects );
        clipper.AddClip( clips );

        const ClipType clipType =
                operation == 0 ? ClipType::Union : ClipType::Difference;

        if( !clipper.Execute( clipType, FillRule::NonZero, *solution ) )
            return nullptr;

        return solution.release();
    }
    catch( ... )
    {
        return nullptr;
    }
}

extern "C" void gerber_clipper_tree_delete( void* tree )
{
    delete static_cast<Clipper2Lib::PolyTree64*>( tree );
}

extern "C" std::size_t gerber_clipper_node_child_count( const void* node )
{
    return static_cast<const Clipper2Lib::PolyPath64*>( node )->Count();
}

extern "C" const void* gerber_clipper_node_child( const void* node, std::size_t index )
{
    const auto* path = static_cast<const Clipper2Lib::PolyPath64*>( node );
    return index < path->Count() ? path->Child( index ) : nullptr;
}

extern "C" std::size_t gerber_clipper_node_point_count( const void* node )
{
    return static_cast<const Clipper2Lib::PolyPath64*>( node )->Polygon().size();
}

extern "C" bool gerber_clipper_node_point( const void* node, std::size_t index,
                                           GERBER_CLIPPER_POINT64* point )
{
    if( !point )
        return false;

    const auto& polygon = static_cast<const Clipper2Lib::PolyPath64*>( node )->Polygon();

    if( index >= polygon.size() )
        return false;

    point->x = polygon[index].x;
    point->y = polygon[index].y;
    return true;
}
