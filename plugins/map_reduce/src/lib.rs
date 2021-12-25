use std::cmp::min;
use std::sync::Arc;

use threadpool::ThreadPool;

const DEFAULT_NUM_WORKERS: usize = 8;
const DEFAULT_QUERY_LIMIT: u32 = u16::max_value() as u32;
const DEFAULT_REDUCER_CHUNK_SIZE: u32 = u8::max_value() as u32;

pub trait MapReduceDriver: Send + Sync + 'static {
    fn num_workers(&self) -> usize {
        DEFAULT_NUM_WORKERS
    }
    fn query_limit(&self) -> u32 {
        DEFAULT_QUERY_LIMIT
    }
    fn reducer_chunk_size(&self) -> u32 {
        DEFAULT_REDUCER_CHUNK_SIZE
    }
    fn t_filter(&self) -> Option<indradb::Identifier> {
        None
    }
    fn map(&self, vertex: indradb::Vertex) -> Result<serde_json::Value, indradb::Error>;
    fn reduce(&self, values: Vec<serde_json::Value>) -> Result<serde_json::Value, indradb::Error>;
}

pub fn map_reduce<D: MapReduceDriver>(
    driver: Arc<D>,
    trans: Box<dyn indradb::Transaction + Send>,
) -> Result<serde_json::Value, indradb::Error> {
    let pool = ThreadPool::new(min(driver.num_workers(), 2));
    let (shutdown_sender, shutdown_receiver) = crossbeam_channel::bounded::<()>(1);
    let (sender, receiver) = crossbeam_channel::unbounded::<Result<serde_json::Value, indradb::Error>>();

    {
        let driver = driver.clone();
        let query_limit = min(driver.query_limit(), 1);
        let t_filter = driver.t_filter();
        let pool_clone = pool.clone();
        let sender = sender.clone();

        pool.execute(move || {
            let mut last_id: Option<uuid::Uuid> = None;

            loop {
                if let Ok(()) = shutdown_receiver.try_recv() {
                    return;
                }

                let q = indradb::RangeVertexQuery {
                    limit: query_limit,
                    t: t_filter.clone(),
                    start_id: last_id,
                };

                let vertices = match trans.get_vertices(q.into()) {
                    Ok(value) => value,
                    Err(err) => {
                        sender.send(Err(err)).unwrap();
                        return;
                    }
                };

                let is_last_query = vertices.len() < query_limit as usize;
                if let Some(last_vertex) = vertices.last() {
                    last_id = Some(last_vertex.id);
                }

                for vertex in vertices {
                    let driver = driver.clone();
                    let sender = sender.clone();
                    pool_clone.execute(move || sender.send(driver.map(vertex)).unwrap());
                }

                if is_last_query {
                    return;
                }
            }
        });
    }

    let reducer_chunk_size = min(driver.reducer_chunk_size() as usize, 2);
    let mut reducibles = Vec::<serde_json::Value>::new();
    let mut final_err = Option::<indradb::Error>::None;
    loop {
        match receiver.recv().unwrap() {
            Ok(value) => reducibles.push(value),
            Err(err) => {
                shutdown_sender.send(()).unwrap();
                final_err = Some(err);
                break;
            }
        };

        let is_idle = pool.active_count() == 0 && receiver.is_empty();

        if reducibles.len() >= reducer_chunk_size || (is_idle && reducibles.len() > 1) {
            let reducibles_chunk: Vec<serde_json::Value> = reducibles.drain(..).collect();
            let driver = driver.clone();
            let sender = sender.clone();
            pool.execute(move || sender.send(driver.reduce(reducibles_chunk)).unwrap());
        } else if is_idle {
            break;
        }
    }

    pool.join();

    if let Some(err) = final_err {
        Err(err)
    } else if let Some(value) = reducibles.pop() {
        Ok(value)
    } else {
        Ok(serde_json::Value::Null)
    }
}
