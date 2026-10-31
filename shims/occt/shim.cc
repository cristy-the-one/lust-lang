/* Open CASCADE first slice. See lust_occt.h and docs/occt-shim.md.
   Not built by scripts/build.sh or by CI. */
#include "lust_occt.h"

#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRep_Tool.hxx>
#include <Poly_Triangulation.hxx>
#include <Standard_Failure.hxx>
#include <TopAbs_Orientation.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Face.hxx>

#include <cstdint>
#include <mutex>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

namespace {

thread_local std::string g_err;

void set_err(const char *s) { g_err.assign(s ? s : ""); }

void clear_err() { g_err.clear(); }

template <class F>
int guard(F fn) {
    try {
        clear_err();
        return fn();
    } catch (const Standard_Failure &e) {
        const char *m = e.GetMessageString();
        if (m && *m) {
            set_err(m);
        } else if (const Handle(Standard_Type) &t = e.DynamicType()) {
            set_err(t->Name());
        } else {
            set_err("OCCT failure");
        }
        return 1;
    } catch (const std::exception &e) {
        set_err(e.what());
        return 2;
    } catch (...) {
        set_err("unknown C++ exception");
        return 3;
    }
}

struct ShapeEnt {
    uint64_t session;
    TopoDS_Shape shape;
};

struct MeshEnt {
    std::vector<double> verts;
    std::vector<std::uint32_t> tris;
};

std::mutex g_mu;
uint64_t g_next_session = 1;
uint64_t g_next_shape = 1;
uint64_t g_next_mesh = 1;
std::unordered_map<uint64_t, char> g_sessions;
std::unordered_map<uint64_t, ShapeEnt> g_shapes;
std::unordered_map<uint64_t, MeshEnt> g_meshes;

void drop_session_locked(uint64_t session) {
    std::vector<uint64_t> dead;
    for (const auto &kv : g_shapes) {
        if (kv.second.session == session) dead.push_back(kv.first);
    }
    for (uint64_t id : dead) g_shapes.erase(id);
    g_sessions.erase(session);
}

int bad(const char *msg) {
    set_err(msg);
    return 4;
}

}  // namespace

extern "C" int lust_occt_session_new(uint64_t *out_session) {
    return guard([&]() -> int {
        if (!out_session) return bad("null out");
        std::lock_guard<std::mutex> lock(g_mu);
        uint64_t id = g_next_session++;
        g_sessions.emplace(id, 1);
        *out_session = id;
        return 0;
    });
}

extern "C" void lust_occt_session_free(uint64_t session) {
    guard([&]() -> int {
        std::lock_guard<std::mutex> lock(g_mu);
        if (!g_sessions.count(session)) return bad("bad handle");
        drop_session_locked(session);
        return 0;
    });
}

extern "C" int lust_occt_make_box(uint64_t session, double dx, double dy, double dz, uint64_t *out_shape) {
    return guard([&]() -> int {
        if (!out_shape) return bad("null out");
        {
            std::lock_guard<std::mutex> lock(g_mu);
            if (!g_sessions.count(session)) return bad("bad handle");
        }
        BRepPrimAPI_MakeBox mk(dx, dy, dz);
        mk.Build();
        if (!mk.IsDone()) {
            set_err("OCCT operation failed");
            return 5;
        }
        TopoDS_Shape shape = mk.Shape();
        std::lock_guard<std::mutex> lock(g_mu);
        if (!g_sessions.count(session)) return bad("bad handle");
        uint64_t id = g_next_shape++;
        g_shapes.emplace(id, ShapeEnt{session, std::move(shape)});
        *out_shape = id;
        return 0;
    });
}

extern "C" int lust_occt_make_cylinder(uint64_t session, double radius, double height, uint64_t *out_shape) {
    return guard([&]() -> int {
        if (!out_shape) return bad("null out");
        {
            std::lock_guard<std::mutex> lock(g_mu);
            if (!g_sessions.count(session)) return bad("bad handle");
        }
        BRepPrimAPI_MakeCylinder mk(radius, height);
        mk.Build();
        if (!mk.IsDone()) {
            set_err("OCCT operation failed");
            return 5;
        }
        TopoDS_Shape shape = mk.Shape();
        std::lock_guard<std::mutex> lock(g_mu);
        if (!g_sessions.count(session)) return bad("bad handle");
        uint64_t id = g_next_shape++;
        g_shapes.emplace(id, ShapeEnt{session, std::move(shape)});
        *out_shape = id;
        return 0;
    });
}

extern "C" void lust_occt_shape_free(uint64_t session, uint64_t shape) {
    guard([&]() -> int {
        std::lock_guard<std::mutex> lock(g_mu);
        auto it = g_shapes.find(shape);
        if (it == g_shapes.end() || it->second.session != session) return bad("bad handle");
        g_shapes.erase(it);
        return 0;
    });
}

extern "C" int lust_occt_tessellate(uint64_t session, uint64_t shape, double linear_deflection, uint64_t *out_mesh) {
    return guard([&]() -> int {
        if (!out_mesh) return bad("null out");
        TopoDS_Shape solid;
        {
            std::lock_guard<std::mutex> lock(g_mu);
            if (!g_sessions.count(session)) return bad("bad handle");
            auto it = g_shapes.find(shape);
            if (it == g_shapes.end() || it->second.session != session) return bad("bad handle");
            solid = it->second.shape;
        }
        /* Single-threaded. The constructor performs the mesh. */
        BRepMesh_IncrementalMesh mesher(solid, linear_deflection, Standard_False, 0.5, Standard_False);
        if (!mesher.IsDone()) {
            set_err("OCCT operation failed");
            return 5;
        }
        MeshEnt mesh;
        for (TopExp_Explorer exp(solid, TopAbs_FACE); exp.More(); exp.Next()) {
            TopoDS_Face face = TopoDS::Face(exp.Current());
            TopLoc_Location loc;
            Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(face, loc);
            if (tri.IsNull() || tri->NbNodes() <= 0 || tri->NbTriangles() <= 0) continue;
            gp_Trsf tr = loc.Transformation();
            uint32_t base = static_cast<uint32_t>(mesh.verts.size() / 3);
            for (int i = 1; i <= tri->NbNodes(); ++i) {
                gp_Pnt p = tri->Node(i);
                p.Transform(tr);
                mesh.verts.push_back(p.X());
                mesh.verts.push_back(p.Y());
                mesh.verts.push_back(p.Z());
            }
            bool rev = face.Orientation() == TopAbs_REVERSED;
            for (int i = 1; i <= tri->NbTriangles(); ++i) {
                int n1 = 0, n2 = 0, n3 = 0;
                tri->Triangle(i).Get(n1, n2, n3);
                uint32_t a = base + static_cast<uint32_t>(n1 - 1);
                uint32_t b = base + static_cast<uint32_t>(n2 - 1);
                uint32_t c = base + static_cast<uint32_t>(n3 - 1);
                if (rev) std::swap(b, c);
                mesh.tris.push_back(a);
                mesh.tris.push_back(b);
                mesh.tris.push_back(c);
            }
        }
        std::lock_guard<std::mutex> lock(g_mu);
        if (!g_sessions.count(session) || !g_shapes.count(shape)) return bad("bad handle");
        uint64_t id = g_next_mesh++;
        g_meshes.emplace(id, std::move(mesh));
        *out_mesh = id;
        return 0;
    });
}

extern "C" int lust_occt_mesh_counts(uint64_t mesh, uint64_t *n_verts, uint64_t *n_tris) {
    return guard([&]() -> int {
        if (!n_verts || !n_tris) return bad("null out");
        std::lock_guard<std::mutex> lock(g_mu);
        auto it = g_meshes.find(mesh);
        if (it == g_meshes.end()) return bad("bad handle");
        *n_verts = it->second.verts.size() / 3;
        *n_tris = it->second.tris.size() / 3;
        return 0;
    });
}

extern "C" int lust_occt_mesh_copy(uint64_t mesh, double *verts, uint32_t *tris) {
    return guard([&]() -> int {
        std::lock_guard<std::mutex> lock(g_mu);
        auto it = g_meshes.find(mesh);
        if (it == g_meshes.end()) return bad("bad handle");
        const MeshEnt &m = it->second;
        if (!m.verts.empty()) {
            if (!verts) return bad("null out");
            for (size_t i = 0; i < m.verts.size(); ++i) verts[i] = m.verts[i];
        }
        if (!m.tris.empty()) {
            if (!tris) return bad("null out");
            for (size_t i = 0; i < m.tris.size(); ++i) tris[i] = m.tris[i];
        }
        return 0;
    });
}

extern "C" void lust_occt_mesh_free(uint64_t mesh) {
    guard([&]() -> int {
        std::lock_guard<std::mutex> lock(g_mu);
        if (!g_meshes.erase(mesh)) return bad("bad handle");
        return 0;
    });
}

extern "C" const char *lust_occt_last_error(void) {
    return g_err.c_str();
}
