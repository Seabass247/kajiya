use kajiya_asset::mesh::{PackedTriMesh, MeshMaterialMap, TexParams, PackedVertex, FormattedTriangleMesh, format_triangle_mesh};
use kajiya_asset::mesh::{pack_triangle_mesh, GpuImage, LoadGltfScene};
use glam::Quat;
use turbosloth::*;
use kajiya_asset::image::{CreatePlaceholderImage, CreateGpuImage};

use crate::world_renderer::{AddMeshOptions, MeshHandle, WorldRenderer};

impl WorldRenderer {
    pub fn add_baked_mesh(
        &mut self,
        path: impl Into<std::path::PathBuf>,
        opts: AddMeshOptions,
    ) -> anyhow::Result<MeshHandle> {
        Ok(self.add_mesh(
            crate::mmap::mmapped_asset::<PackedTriMesh::Flat, _>(path)?,
            opts,
        ))
    }

    pub fn load_gltf_mesh(
        &mut self,
        path: impl Into<std::path::PathBuf>,
        scale: f32,
        opts: AddMeshOptions,
    ) -> anyhow::Result<MeshHandle> {
        let mesh = LoadGltfScene {
            path: path.into(),
            scale,
            rotation: Quat::IDENTITY,
        }.load()?;

        Ok(self.add_runtime_mesh(
            format_triangle_mesh(&mesh),
            opts,
        ))
    }
}
