use super::super::bytes::*;
use crate::errors::Result;
use crate::models;
use chrono::offset::Utc;
use chrono::DateTime;
use serde_json::Value as JsonValue;
use sled::Result as SledResult;
use sled::{IVec, Iter as DbIterator, Tree, Batch, Transactional, Config, Db};
use std::io::Cursor;
use std::ops::Deref;
use std::u8;
use uuid::Uuid;

pub type OwnedPropertyItem = ((Uuid, String), JsonValue);
pub type VertexItem = (Uuid, models::Type);
pub type EdgeRangeItem = (Uuid, models::Type, DateTime<Utc>, Uuid);
pub type EdgePropertyItem = ((Uuid, models::Type, Uuid, String), JsonValue);

fn take_while_prefixed(iterator: DbIterator, prefix: Vec<u8>) -> impl Iterator<Item = SledResult<(IVec, IVec)>> {
    iterator.take_while(move |item| -> bool {
        match item {
            Ok((k, _)) => k.starts_with(&prefix),
            Err(_) => false,
        }
    })
}

#[derive(Copy, Clone, Default, Debug)]
pub struct SledConfig {
    pub(crate) use_compression: bool,
    pub(crate) compression_factor: Option<i32>,
}

impl SledConfig {
    /// Creates a new sled config with zstd compression enabled.
    ///
    /// # Arguments
    /// * `factor` - The zstd compression factor to use. If unspecified, this
    ///   will default to 5.
    pub fn with_compression(factor: Option<i32>) -> SledConfig {
        return SledConfig {
            use_compression: true,
            compression_factor: factor,
        };
    }
}

/// The meat of a Sled datastore
pub struct SledHolder {
    pub(crate) db: Db,
    pub(crate) vertices: Tree,
    pub(crate) edges: Tree,
    pub(crate) edge_ranges: Tree,
    pub(crate) reversed_edge_ranges: Tree,
    pub(crate) vertex_properties: Tree,
    pub(crate) edge_properties: Tree,
}

impl<'ds> SledHolder {
    /// The meat of a Sled datastore.
    ///
    /// # Arguments
    /// * `path` - The file path to the Sled database.
    /// * `config` - Sled options to pass in.
    pub fn new(path: &str, config: SledConfig) -> Result<SledHolder> {
        let mut sled_config = Config::default().path(path);

        if config.use_compression {
            sled_config = sled_config.use_compression(true);
        }

        if let Some(compression_factor) = config.compression_factor {
            sled_config = sled_config.compression_factor(compression_factor);
        }

        let db = sled_config.open()?;

        Ok(SledHolder {
            vertices: db.open_tree("vertices")?,
            edges: db.open_tree("edges")?,
            edge_ranges: db.open_tree("edge_ranges")?,
            reversed_edge_ranges: db.open_tree("reversed_edge_ranges")?,
            vertex_properties: db.open_tree("vertex_properties")?,
            edge_properties: db.open_tree("edge_properties")?,
            db,
        })
    }
}

#[derive(Default)]
pub(crate) struct UberBatch {
    pub vertices: Option<Batch>,
    pub edges: Option<Batch>,
    pub edge_ranges: Option<Batch>,
    pub reversed_edge_ranges: Option<Batch>,
    pub vertex_properties: Option<Batch>,
    pub edge_properties: Option<Batch>,
}

impl UberBatch {
    pub(crate) fn vertices(&mut self) -> &mut Batch {
        if self.vertices.is_none() {
            self.vertices = Some(Batch::default());
        }
        self.vertices.as_mut().unwrap()
    }

    pub(crate) fn edges(&mut self) -> &mut Batch {
        if self.edges.is_none() {
            self.edges = Some(Batch::default());
        }
        self.edges.as_mut().unwrap()
    }

    pub(crate) fn edge_ranges(&mut self) -> &mut Batch {
        if self.edge_ranges.is_none() {
            self.edge_ranges = Some(Batch::default());
        }
        self.edge_ranges.as_mut().unwrap()
    }

    pub(crate) fn reversed_edge_ranges(&mut self) -> &mut Batch {
        if self.reversed_edge_ranges.is_none() {
            self.reversed_edge_ranges = Some(Batch::default());
        }
        self.reversed_edge_ranges.as_mut().unwrap()
    }

    pub(crate) fn vertex_properties(&mut self) -> &mut Batch {
        if self.vertex_properties.is_none() {
            self.vertex_properties = Some(Batch::default());
        }
        self.vertex_properties.as_mut().unwrap()
    }

    pub(crate) fn edge_properties(&mut self) -> &mut Batch {
        if self.edge_properties.is_none() {
            self.edge_properties = Some(Batch::default());
        }
        self.edge_properties.as_mut().unwrap()
    }

    pub(crate) fn apply(self, holder: &SledHolder) -> Result<()> {
        // TODO: find a better way to do this that minimizes the number of
        // transactions
        let trees = (
            &holder.vertices,
            &holder.edges,
            &holder.edge_ranges,
            &holder.reversed_edge_ranges,
            &holder.vertex_properties,
            &holder.edge_properties,
        );

        trees.transaction(|(vertices_tree, edges_tree, edge_ranges_tree, reversed_edge_ranges_tree, vertex_properties_tree, edge_properties_tree)| {
            if let Some(vertices_batch) = &self.vertices {
                vertices_tree.apply_batch(&vertices_batch)?;
            }
            if let Some(edges_batch) = &self.edges {
                edges_tree.apply_batch(&edges_batch)?;
            }
            if let Some(edge_ranges_batch) = &self.edge_ranges {
                edge_ranges_tree.apply_batch(&edge_ranges_batch)?;
            }
            if let Some(reversed_edge_ranges_batch) = &self.reversed_edge_ranges {
                reversed_edge_ranges_tree.apply_batch(&reversed_edge_ranges_batch)?;
            }
            if let Some(vertex_properties_batch) = &self.vertex_properties {
                vertex_properties_tree.apply_batch(&vertex_properties_batch)?;
            }
            if let Some(edge_properties_batch) = &self.edge_properties {
                edge_properties_tree.apply_batch(&edge_properties_batch)?;
            }
            Ok(())
        })?;

        Ok(())
    }
}

pub(crate) struct VertexManager<'db: 'tree, 'tree> {
    pub holder: &'db SledHolder,
    pub tree: &'tree Tree,
}

impl<'db: 'tree, 'tree> VertexManager<'db, 'tree> {
    pub fn new(ds: &'db SledHolder) -> Self {
        VertexManager {
            holder: ds,
            tree: &ds.vertices,
        }
    }

    fn key(&self, id: Uuid) -> IVec {
        build(&[Component::Uuid(id)]).into()
    }

    pub fn exists(&self, id: Uuid) -> Result<bool> {
        Ok(self.tree.get(&self.key(id))?.is_some())
    }

    pub fn get(&self, id: Uuid) -> Result<Option<models::Type>> {
        match self.tree.get(&self.key(id))? {
            Some(value_bytes) => {
                let mut cursor = Cursor::new(value_bytes.deref());
                Ok(Some(read_type(&mut cursor)))
            }
            None => Ok(None),
        }
    }

    fn iterate(&self, iterator: DbIterator) -> impl Iterator<Item = Result<VertexItem>> + '_ {
        iterator.map(move |item| -> Result<VertexItem> {
            let (k, v) = item?;

            let id = {
                debug_assert_eq!(k.len(), 16);
                let mut cursor = Cursor::new(k);
                read_uuid(&mut cursor)
            };

            let mut cursor = Cursor::new(v);
            let t = read_type(&mut cursor);
            Ok((id, t))
        })
    }

    pub fn iterate_for_range<'a>(&'a self, id: Uuid) -> impl Iterator<Item = Result<VertexItem>> + 'a {
        let low_key = build(&[Component::Uuid(id)]);
        let low_key_bytes: &[u8] = low_key.as_ref();
        let iter = self.tree.range(low_key_bytes..);
        self.iterate(iter)
    }

    pub fn create(&self, vertex: &models::Vertex) -> Result<()> {
        let key = self.key(vertex.id);
        self.tree.insert(&key, build(&[Component::Type(&vertex.t)]))?;
        Ok(())
    }

    pub fn delete(&self, batch: &mut UberBatch, id: Uuid) -> Result<()> {
        batch.vertices().remove(&self.key(id));

        let vertex_property_manager = VertexPropertyManager::new(&self.holder);
        for item in vertex_property_manager.iterate_for_owner(id) {
            let ((vertex_property_owner_id, vertex_property_name), _) = item?;
            vertex_property_manager.delete(batch, vertex_property_owner_id, &vertex_property_name[..]);
        }

        let edge_manager = EdgeManager::new(&self.holder);

        {
            let edge_range_manager = EdgeRangeManager::new(&self.holder);
            for item in edge_range_manager.iterate_for_owner(id) {
                let (edge_range_outbound_id, edge_range_t, edge_range_update_datetime, edge_range_inbound_id) = item?;
                debug_assert_eq!(edge_range_outbound_id, id);
                edge_manager.delete(
                    batch,
                    edge_range_outbound_id,
                    &edge_range_t,
                    edge_range_inbound_id,
                    edge_range_update_datetime,
                )?;
            }
        }

        {
            let reversed_edge_range_manager = EdgeRangeManager::new_reversed(&self.holder);
            for item in reversed_edge_range_manager.iterate_for_owner(id) {
                let (
                    reversed_edge_range_inbound_id,
                    reversed_edge_range_t,
                    reversed_edge_range_update_datetime,
                    reversed_edge_range_outbound_id,
                ) = item?;
                debug_assert_eq!(reversed_edge_range_inbound_id, id);
                edge_manager.delete(
                    batch,
                    reversed_edge_range_outbound_id,
                    &reversed_edge_range_t,
                    reversed_edge_range_inbound_id,
                    reversed_edge_range_update_datetime,
                )?;
            }
        }
        Ok(())
    }
}

pub(crate) struct EdgeManager<'db: 'tree, 'tree> {
    pub holder: &'db SledHolder,
    pub tree: &'tree Tree,
}

impl<'db, 'tree> EdgeManager<'db, 'tree> {
    pub fn new(ds: &'db SledHolder) -> Self {
        EdgeManager {
            holder: ds,
            tree: &ds.edges,
        }
    }

    fn key(&self, outbound_id: Uuid, t: &models::Type, inbound_id: Uuid) -> IVec {
        build(&[
            Component::Uuid(outbound_id),
            Component::Type(t),
            Component::Uuid(inbound_id),
        ]).into()
    }

    pub fn get(&self, outbound_id: Uuid, t: &models::Type, inbound_id: Uuid) -> Result<Option<DateTime<Utc>>> {
        match self.tree.get(self.key(outbound_id, t, inbound_id))? {
            Some(value_bytes) => {
                let mut cursor = Cursor::new(value_bytes.deref());
                Ok(Some(read_datetime(&mut cursor)))
            }
            None => Ok(None),
        }
    }

    pub fn set(
        &self,
        batch: &mut UberBatch,
        outbound_id: Uuid,
        t: &models::Type,
        inbound_id: Uuid,
        new_update_datetime: DateTime<Utc>,
    ) -> Result<()> {
        let edge_range_manager = EdgeRangeManager::new(&self.holder);
        let reversed_edge_range_manager = EdgeRangeManager::new_reversed(&self.holder);

        if let Some(update_datetime) = self.get(outbound_id, t, inbound_id)? {
            edge_range_manager.delete(batch, outbound_id, t, update_datetime, inbound_id);
            reversed_edge_range_manager.delete(batch, inbound_id, t, update_datetime, outbound_id);
        }

        let key = self.key(outbound_id, t, inbound_id);
        batch.edges().insert(key, build(&[Component::DateTime(new_update_datetime)]));
        edge_range_manager.set(batch, outbound_id, t, new_update_datetime, inbound_id);
        reversed_edge_range_manager.set(batch, inbound_id, t, new_update_datetime, outbound_id);
        Ok(())
    }

    pub fn delete(
        &self,
        batch: &mut UberBatch,
        outbound_id: Uuid,
        t: &models::Type,
        inbound_id: Uuid,
        update_datetime: DateTime<Utc>,
    ) -> Result<()> {
        batch.edges().remove(&self.key(outbound_id, t, inbound_id));

        let edge_range_manager = EdgeRangeManager::new(&self.holder);
        edge_range_manager.delete(batch, outbound_id, t, update_datetime, inbound_id);

        let reversed_edge_range_manager = EdgeRangeManager::new_reversed(&self.holder);
        reversed_edge_range_manager.delete(batch, inbound_id, t, update_datetime, outbound_id);

        let edge_property_manager = EdgePropertyManager::new(&self.holder);
        for item in edge_property_manager.iterate_for_owner(outbound_id, t, inbound_id) {
            let ((edge_property_outbound_id, edge_property_t, edge_property_inbound_id, edge_property_name), _) = item?;
            edge_property_manager.delete(
                batch,
                edge_property_outbound_id,
                &edge_property_t,
                edge_property_inbound_id,
                &edge_property_name[..],
            );
        }
        Ok(())
    }
}

pub(crate) struct EdgeRangeManager<'tree> {
    pub tree: &'tree Tree,
    reversed: bool
}

impl<'tree> EdgeRangeManager<'tree> {
    pub fn new<'db: 'tree>(ds: &'db SledHolder) -> Self {
        EdgeRangeManager { tree: &ds.edge_ranges, reversed: false }
    }

    pub fn new_reversed<'db: 'tree>(ds: &'db SledHolder) -> Self {
        EdgeRangeManager {
            tree: &ds.reversed_edge_ranges,
            reversed: true,
        }
    }

    fn key(&self, first_id: Uuid, t: &models::Type, update_datetime: DateTime<Utc>, second_id: Uuid) -> IVec {
        build(&[
            Component::Uuid(first_id),
            Component::Type(t),
            Component::DateTime(update_datetime),
            Component::Uuid(second_id),
        ]).into()
    }

    fn iterate<'it>(
        &self,
        iterator: DbIterator,
        prefix: Vec<u8>,
    ) -> impl Iterator<Item = Result<EdgeRangeItem>> + 'it {
        let filtered = take_while_prefixed(iterator, prefix);
        filtered.map(move |item| -> Result<EdgeRangeItem> {
            let (k, _) = item?;
            let mut cursor = Cursor::new(k);
            let first_id = read_uuid(&mut cursor);
            let t = read_type(&mut cursor);
            let update_datetime = read_datetime(&mut cursor);
            let second_id = read_uuid(&mut cursor);
            Ok((first_id, t, update_datetime, second_id))
        })
    }

    pub fn iterate_for_range<'iter, 'trans: 'iter>(
        &'trans self,
        id: Uuid,
        t: Option<&models::Type>,
        high: Option<DateTime<Utc>>,
    ) -> Box<dyn Iterator<Item = Result<EdgeRangeItem>> + 'iter> {
        match t {
            Some(t) => {
                let high = high.unwrap_or_else(|| *MAX_DATETIME);
                let prefix = build(&[Component::Uuid(id), Component::Type(t)]);
                let low_key = build(&[Component::Uuid(id), Component::Type(t), Component::DateTime(high)]);
                let low_key_bytes: &[u8] = low_key.as_ref();
                let iterator = self.tree.range(low_key_bytes..);
                Box::new(self.iterate(iterator, prefix))
            }
            None => {
                let prefix = build(&[Component::Uuid(id)]);
                let prefix_bytes: &[u8] = prefix.as_ref();
                let iterator = self.tree.range(prefix_bytes..);
                let mapped = self.iterate(iterator, prefix);

                if let Some(high) = high {
                    // We can filter out `update_datetime`s greater than
                    // `high` via key prefix filtering, so instead we handle
                    // it here - after the key has been deserialized.
                    let filtered = mapped.filter(move |item| {
                        if let Ok((_, _, update_datetime, _)) = *item {
                            update_datetime <= high
                        } else {
                            true
                        }
                    });

                    Box::new(filtered)
                } else {
                    Box::new(mapped)
                }
            }
        }
    }

    pub fn iterate_for_owner<'iter, 'trans: 'iter>(
        &'trans self,
        id: Uuid,
    ) -> impl Iterator<Item = Result<EdgeRangeItem>> + 'iter {
        let prefix: Vec<u8> = build(&[Component::Uuid(id)]);
        let iterator = self.tree.scan_prefix(&prefix);
        self.iterate(iterator, prefix)
    }

    pub fn set(&self, batch: &mut UberBatch, first_id: Uuid, t: &models::Type, update_datetime: DateTime<Utc>, second_id: Uuid) {
        let key = self.key(first_id, t, update_datetime, second_id);
        if self.reversed {
            batch.reversed_edge_ranges().insert(&key, &[]);
        } else {
            batch.edge_ranges().insert(&key, &[]);
        }
    }

    pub fn delete(
        &self,
        batch: &mut UberBatch,
        first_id: Uuid,
        t: &models::Type,
        update_datetime: DateTime<Utc>,
        second_id: Uuid,
    ) {
        if self.reversed {
            batch.reversed_edge_ranges().remove(&self.key(first_id, t, update_datetime, second_id));
        } else {
            batch.edge_ranges().remove(&self.key(first_id, t, update_datetime, second_id));
        }
    }
}

pub(crate) struct VertexPropertyManager<'tree> {
    pub tree: &'tree Tree,
}

impl<'tree> VertexPropertyManager<'tree> {
    pub fn new<'db: 'tree>(ds: &'db SledHolder) -> Self {
        VertexPropertyManager { tree: &ds.vertex_properties }
    }


    fn key(&self, vertex_id: Uuid, name: &str) -> IVec {
        build(&[Component::Uuid(vertex_id), Component::UnsizedString(name)]).into()
    }

    pub fn iterate_for_owner(&self, vertex_id: Uuid) -> impl Iterator<Item = Result<OwnedPropertyItem>> + '_ {
        let prefix = build(&[Component::Uuid(vertex_id)]);
        let iterator = self.tree.scan_prefix(&prefix);

        iterator.map(move |item| -> Result<OwnedPropertyItem> {
            let (k, v) = item?;
            let mut cursor = Cursor::new(k);
            let owner_id = read_uuid(&mut cursor);
            debug_assert_eq!(vertex_id, owner_id);
            let name = read_unsized_string(&mut cursor);
            let value = serde_json::from_slice(&v)?;
            Ok(((owner_id, name), value))
        })
    }

    pub fn get(&self, vertex_id: Uuid, name: &str) -> Result<Option<JsonValue>> {
        let key = self.key(vertex_id, name);

        match self.tree.get(&key)? {
            Some(value_bytes) => Ok(Some(serde_json::from_slice(&value_bytes)?)),
            None => Ok(None),
        }
    }

    pub fn set(&self, vertex_id: Uuid, name: &str, value: &JsonValue) -> Result<()> {
        let key = self.key(vertex_id, name);
        let value_json = serde_json::to_vec(value)?;
        self.tree.insert(key, value_json.as_slice())?;
        Ok(())
    }

    pub fn delete(&self, batch: &mut UberBatch, vertex_id: Uuid, name: &str) {
        batch.vertex_properties().remove(&self.key(vertex_id, name));
    }
}

pub(crate) struct EdgePropertyManager<'tree> {
    pub tree: &'tree Tree,
}

impl<'tree> EdgePropertyManager<'tree> {
    pub fn new<'db: 'tree>(ds: &'db SledHolder) -> Self {
        EdgePropertyManager { tree: &ds.edge_properties }
    }

    fn key(&self, outbound_id: Uuid, t: &models::Type, inbound_id: Uuid, name: &str) -> IVec {
        build(&[
            Component::Uuid(outbound_id),
            Component::Type(t),
            Component::Uuid(inbound_id),
            Component::UnsizedString(name),
        ]).into()
    }

    pub fn iterate_for_owner<'a>(
        &'a self,
        outbound_id: Uuid,
        t: &'a models::Type,
        inbound_id: Uuid,
    ) -> Box<dyn Iterator<Item = Result<EdgePropertyItem>> + 'a> {
        let prefix = build(&[
            Component::Uuid(outbound_id),
            Component::Type(t),
            Component::Uuid(inbound_id),
        ]);

        let iterator = self.tree.scan_prefix(&prefix);

        let mapped = iterator.map(move |item| -> Result<EdgePropertyItem> {
            let (k, v) = item?;
            let mut cursor = Cursor::new(k);

            let edge_property_outbound_id = read_uuid(&mut cursor);
            debug_assert_eq!(edge_property_outbound_id, outbound_id);

            let edge_property_t = read_type(&mut cursor);
            debug_assert_eq!(&edge_property_t, t);

            let edge_property_inbound_id = read_uuid(&mut cursor);
            debug_assert_eq!(edge_property_inbound_id, inbound_id);

            let edge_property_name = read_unsized_string(&mut cursor);

            let value = serde_json::from_slice(&v)?;
            Ok((
                (
                    edge_property_outbound_id,
                    edge_property_t,
                    edge_property_inbound_id,
                    edge_property_name,
                ),
                value,
            ))
        });

        Box::new(mapped)
    }

    pub fn get(&self, outbound_id: Uuid, t: &models::Type, inbound_id: Uuid, name: &str) -> Result<Option<JsonValue>> {
        let key = self.key(outbound_id, t, inbound_id, name);

        match self.tree.get(&key)? {
            Some(ref value_bytes) => Ok(Some(serde_json::from_slice(&value_bytes)?)),
            None => Ok(None),
        }
    }

    pub fn set(
        &self,
        outbound_id: Uuid,
        t: &models::Type,
        inbound_id: Uuid,
        name: &str,
        value: &JsonValue,
    ) -> Result<()> {
        let key = self.key(outbound_id, t, inbound_id, name);
        let value_json = serde_json::to_vec(value)?;
        self.tree.insert(key, value_json.as_slice())?;
        Ok(())
    }

    pub fn delete(&self, batch: &mut UberBatch, outbound_id: Uuid, t: &models::Type, inbound_id: Uuid, name: &str) {
        batch.edge_properties().remove(&self.key(outbound_id, t, inbound_id, name));
    }
}