use pyo3::prelude::*;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::types::{PyBytes, PyDict};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Error conversion
// ---------------------------------------------------------------------------

fn to_py_err(e: rosetta_core::RosettaError) -> PyErr {
    PyRuntimeError::new_err(format!("{e}"))
}

// ---------------------------------------------------------------------------
// Dtype
// ---------------------------------------------------------------------------

#[pyclass(eq, eq_int)]
#[derive(Clone, PartialEq)]
enum PyDtype {
    Float32,
    Float64,
    Int32,
    Int64,
    Uint8,
    Bool,
    String,
}

impl From<rosetta_core::Dtype> for PyDtype {
    fn from(d: rosetta_core::Dtype) -> Self {
        match d {
            rosetta_core::Dtype::Float32 => PyDtype::Float32,
            rosetta_core::Dtype::Float64 => PyDtype::Float64,
            rosetta_core::Dtype::Int32 => PyDtype::Int32,
            rosetta_core::Dtype::Int64 => PyDtype::Int64,
            rosetta_core::Dtype::Uint8 => PyDtype::Uint8,
            rosetta_core::Dtype::Bool => PyDtype::Bool,
            rosetta_core::Dtype::String => PyDtype::String,
        }
    }
}

// ---------------------------------------------------------------------------
// FieldSchema
// ---------------------------------------------------------------------------

#[pyclass(name = "FieldSchema")]
#[derive(Clone)]
struct PyFieldSchema {
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    shape: Vec<usize>,
    #[pyo3(get)]
    dtype: PyDtype,
    #[pyo3(get)]
    description: Option<String>,
}

impl From<&rosetta_core::FieldSchema> for PyFieldSchema {
    fn from(f: &rosetta_core::FieldSchema) -> Self {
        Self {
            name: f.name.clone(),
            shape: f.shape.clone(),
            dtype: f.dtype.into(),
            description: f.description.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// CameraInfo
// ---------------------------------------------------------------------------

#[pyclass(name = "CameraInfo")]
#[derive(Clone)]
struct PyCameraInfo {
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    height: u32,
    #[pyo3(get)]
    width: u32,
    #[pyo3(get)]
    channels: u8,
    #[pyo3(get)]
    encoding: String,
}

impl From<&rosetta_core::CameraInfo> for PyCameraInfo {
    fn from(c: &rosetta_core::CameraInfo) -> Self {
        Self {
            name: c.name.clone(),
            height: c.height,
            width: c.width,
            channels: c.channels,
            encoding: c.encoding.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// DatasetInfo
// ---------------------------------------------------------------------------

#[pyclass(name = "DatasetInfo")]
#[derive(Clone)]
struct PyDatasetInfo {
    #[pyo3(get)]
    format: String,
    #[pyo3(get)]
    path: String,
    #[pyo3(get)]
    total_episodes: usize,
    #[pyo3(get)]
    total_frames: usize,
    #[pyo3(get)]
    fps: Option<f64>,
    #[pyo3(get)]
    robot_type: Option<String>,
    #[pyo3(get)]
    fields: Vec<PyFieldSchema>,
    #[pyo3(get)]
    cameras: Vec<PyCameraInfo>,
}

impl From<&rosetta_core::DatasetInfo> for PyDatasetInfo {
    fn from(d: &rosetta_core::DatasetInfo) -> Self {
        Self {
            format: d.format.clone(),
            path: d.path.display().to_string(),
            total_episodes: d.total_episodes,
            total_frames: d.total_frames,
            fps: d.fps,
            robot_type: d.robot_type.clone(),
            fields: d.fields.iter().map(PyFieldSchema::from).collect(),
            cameras: d.cameras.iter().map(PyCameraInfo::from).collect(),
        }
    }
}

#[pymethods]
impl PyDatasetInfo {
    fn __repr__(&self) -> String {
        format!(
            "DatasetInfo(format='{}', episodes={}, frames={}, cameras={})",
            self.format, self.total_episodes, self.total_frames, self.cameras.len()
        )
    }
}

// ---------------------------------------------------------------------------
// Frame — holds a shared reference back to the owning Episode
// ---------------------------------------------------------------------------

#[pyclass(name = "Frame")]
struct PyFrame {
    episode: Arc<rosetta_core::Episode>,
    index: usize,
}

impl PyFrame {
    fn inner(&self) -> &rosetta_core::Frame {
        &self.episode.frames[self.index]
    }
}

#[pymethods]
impl PyFrame {
    #[getter]
    fn timestep(&self) -> usize {
        self.inner().timestep
    }

    #[getter]
    fn image_names(&self) -> Vec<String> {
        self.inner().image_names().into_iter().map(|s| s.to_string()).collect()
    }

    fn has_image(&self, camera_name: &str) -> bool {
        self.inner().has_image(camera_name)
    }

    /// Get image as raw bytes for a given camera name.
    fn image_bytes<'py>(&self, py: Python<'py>, camera_name: &str) -> PyResult<Bound<'py, PyBytes>> {
        let frame = self.inner();
        let img = frame.images.get(camera_name)
            .ok_or_else(|| PyValueError::new_err(format!("No image for camera '{camera_name}'")))?;
        let data = img.load().map_err(to_py_err)?;
        Ok(PyBytes::new(py, data))
    }

    /// Get image dimensions (height, width, channels) for a given camera.
    fn image_shape(&self, camera_name: &str) -> PyResult<(u32, u32, u8)> {
        let frame = self.inner();
        let img = frame.images.get(camera_name)
            .ok_or_else(|| PyValueError::new_err(format!("No image for camera '{camera_name}'")))?;
        Ok(img.shape())
    }

    /// Get state as a list of f64.
    #[getter]
    fn state(&self) -> PyResult<Option<Vec<f64>>> {
        match &self.inner().state {
            Some(arr) => {
                let data = arr.load().map_err(to_py_err)?;
                Ok(Some(data.clone()))
            }
            None => Ok(None),
        }
    }

    /// Get action as a list of f64.
    #[getter]
    fn action(&self) -> PyResult<Option<Vec<f64>>> {
        match &self.inner().action {
            Some(arr) => {
                let data = arr.load().map_err(to_py_err)?;
                Ok(Some(data.clone()))
            }
            None => Ok(None),
        }
    }

    /// Get metadata as a JSON-compatible dict.
    fn metadata<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (k, v) in &self.inner().metadata {
            dict.set_item(k, v.to_string())?;
        }
        Ok(dict)
    }

    fn __repr__(&self) -> String {
        let frame = self.inner();
        format!(
            "Frame(timestep={}, images={}, has_state={}, has_action={})",
            frame.timestep,
            frame.images.len(),
            frame.state.is_some(),
            frame.action.is_some(),
        )
    }
}

// ---------------------------------------------------------------------------
// Episode — owns data via Arc so frames can borrow
// ---------------------------------------------------------------------------

#[pyclass(name = "Episode")]
struct PyEpisode {
    inner: Arc<rosetta_core::Episode>,
}

#[pymethods]
impl PyEpisode {
    #[getter]
    fn episode_id(&self) -> &str {
        &self.inner.episode_id
    }

    #[getter]
    fn num_frames(&self) -> usize {
        self.inner.frame_count()
    }

    fn frame(&self, index: usize) -> PyResult<PyFrame> {
        if index >= self.inner.frames.len() {
            return Err(PyValueError::new_err(format!(
                "Frame index {index} out of range ({})",
                self.inner.frames.len()
            )));
        }
        Ok(PyFrame {
            episode: Arc::clone(&self.inner),
            index,
        })
    }

    fn __len__(&self) -> usize {
        self.inner.frame_count()
    }

    fn __repr__(&self) -> String {
        format!(
            "Episode(id='{}', frames={})",
            self.inner.episode_id,
            self.inner.frame_count(),
        )
    }
}

// ---------------------------------------------------------------------------
// ConversionResult
// ---------------------------------------------------------------------------

#[pyclass(name = "ConversionResult")]
#[derive(Clone)]
struct PyConversionResult {
    #[pyo3(get)]
    episodes_converted: usize,
    #[pyo3(get)]
    episodes_failed: usize,
    #[pyo3(get)]
    total_frames: usize,
    #[pyo3(get)]
    errors: Vec<String>,
}

#[pymethods]
impl PyConversionResult {
    fn __repr__(&self) -> String {
        format!(
            "ConversionResult(converted={}, failed={}, frames={})",
            self.episodes_converted, self.episodes_failed, self.total_frames
        )
    }
}

// ---------------------------------------------------------------------------
// FormatCapability
// ---------------------------------------------------------------------------

#[pyclass(name = "FormatCapability")]
#[derive(Clone)]
struct PyFormatCapability {
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    can_read: bool,
    #[pyo3(get)]
    can_write: bool,
    #[pyo3(get)]
    description: String,
}

#[pymethods]
impl PyFormatCapability {
    fn __repr__(&self) -> String {
        let rw = match (self.can_read, self.can_write) {
            (true, true) => "read+write",
            (true, false) => "read-only",
            (false, true) => "write-only",
            (false, false) => "none",
        };
        format!("FormatCapability(name='{}', {})", self.name, rw)
    }
}

// ---------------------------------------------------------------------------
// QualityReport wrappers
// ---------------------------------------------------------------------------

#[pyclass(name = "MetricScore")]
#[derive(Clone)]
struct PyMetricScore {
    #[pyo3(get)]
    normalized: Option<f64>,
    #[pyo3(get)]
    raw: Option<f64>,
    #[pyo3(get)]
    warning: Option<String>,
}

#[pyclass(name = "EpisodeQuality")]
#[derive(Clone)]
struct PyEpisodeQuality {
    #[pyo3(get)]
    episode_id: String,
    #[pyo3(get)]
    num_frames: usize,
    #[pyo3(get)]
    composite_score: Option<f64>,
    #[pyo3(get)]
    flags: Vec<String>,
    metrics: HashMap<String, PyMetricScore>,
}

#[pymethods]
impl PyEpisodeQuality {
    #[getter]
    fn metric_scores<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (k, v) in &self.metrics {
            dict.set_item(k, v.clone().into_pyobject(py)?)?;
        }
        Ok(dict)
    }

    fn __repr__(&self) -> String {
        format!(
            "EpisodeQuality(id='{}', score={:?}, flags={})",
            self.episode_id,
            self.composite_score,
            self.flags.len()
        )
    }
}

#[pyclass(name = "QualityReport")]
#[derive(Clone)]
struct PyQualityReport {
    #[pyo3(get)]
    dataset_path: String,
    #[pyo3(get)]
    num_episodes: usize,
    #[pyo3(get)]
    overall_score: f64,
    #[pyo3(get)]
    per_episode: Vec<PyEpisodeQuality>,
    #[pyo3(get)]
    flags: Vec<String>,
    #[pyo3(get)]
    recommendations: Vec<String>,
}

#[pymethods]
impl PyQualityReport {
    fn __repr__(&self) -> String {
        format!(
            "QualityReport(episodes={}, overall_score={:.3})",
            self.num_episodes, self.overall_score
        )
    }
}

// ---------------------------------------------------------------------------
// Module-level functions
// ---------------------------------------------------------------------------

/// List all supported dataset formats.
#[pyfunction]
fn formats() -> Vec<PyFormatCapability> {
    rosetta_formats::all_formats()
        .iter()
        .map(|f| PyFormatCapability {
            name: f.name.to_string(),
            can_read: f.can_read,
            can_write: f.can_write,
            description: f.description.to_string(),
        })
        .collect()
}

/// Auto-detect the format of a dataset at the given path.
#[pyfunction]
fn detect_format(path: &str) -> PyResult<String> {
    rosetta_formats::detect_format(Path::new(path))
        .map(|s| s.to_string())
        .map_err(to_py_err)
}

/// Inspect a dataset and return metadata without loading all data.
#[pyfunction]
fn inspect(path: &str) -> PyResult<PyDatasetInfo> {
    let format_name = rosetta_formats::detect_format(Path::new(path)).map_err(to_py_err)?;
    let reader = rosetta_formats::get_reader(format_name).map_err(to_py_err)?;
    let info = reader.inspect(Path::new(path)).map_err(to_py_err)?;
    Ok(PyDatasetInfo::from(&info))
}

/// Read all episodes from a dataset.
#[pyfunction]
#[pyo3(signature = (path, format=None))]
fn read_episodes(path: &str, format: Option<&str>) -> PyResult<Vec<PyEpisode>> {
    let format_name = match format {
        Some(f) => f.to_string(),
        None => rosetta_formats::detect_format(Path::new(path))
            .map(|s| s.to_string())
            .map_err(to_py_err)?,
    };
    let reader = rosetta_formats::get_reader(&format_name).map_err(to_py_err)?;
    let output = reader.read_episodes(Path::new(path)).map_err(to_py_err)?;
    Ok(output
        .data
        .into_iter()
        .map(|ep| PyEpisode { inner: Arc::new(ep) })
        .collect())
}

/// Convert a dataset from one format to another.
#[pyfunction]
#[pyo3(signature = (source, output, target_format=None, source_format=None, fail_on_error=false, deterministic=false, num_workers=None))]
fn convert(
    source: &str,
    output: &str,
    target_format: Option<&str>,
    source_format: Option<&str>,
    fail_on_error: bool,
    deterministic: bool,
    num_workers: Option<usize>,
) -> PyResult<PyConversionResult> {
    let mut config = rosetta_core::ConversionConfig::default();
    if let Some(sf) = source_format {
        config.source_format = Some(sf.to_string());
    }
    if let Some(tf) = target_format {
        config.target_format = Some(tf.to_string());
    }
    config.fail_on_error = fail_on_error;
    config.deterministic = deterministic;
    if let Some(n) = num_workers {
        config.num_workers = n;
    }

    let converter = rosetta_pipeline::Converter::new(config);
    let result = converter
        .convert(Path::new(source), Path::new(output))
        .map_err(to_py_err)?;

    Ok(PyConversionResult {
        episodes_converted: result.episodes_converted,
        episodes_failed: result.episodes_failed,
        total_frames: result.total_frames,
        errors: result.errors,
    })
}

/// Analyze the quality of a dataset.
#[pyfunction]
fn analyze_quality(path: &str) -> PyResult<PyQualityReport> {
    let format_name = rosetta_formats::detect_format(Path::new(path)).map_err(to_py_err)?;
    let reader = rosetta_formats::get_reader(format_name).map_err(to_py_err)?;
    let config = rosetta_quality::QualityConfig::default();
    let analyzer = rosetta_quality::QualityAnalyzer::new(config);
    let report = analyzer
        .analyze_dataset(reader.as_ref(), Path::new(path))
        .map_err(to_py_err)?;

    Ok(PyQualityReport {
        dataset_path: report.dataset_path.clone(),
        num_episodes: report.num_episodes,
        overall_score: report.overall_score,
        flags: report.flags.clone(),
        recommendations: report.recommendations.clone(),
        per_episode: report
            .per_episode
            .iter()
            .map(|eq| PyEpisodeQuality {
                episode_id: eq.episode_id.clone(),
                num_frames: eq.num_frames,
                composite_score: eq.composite_score,
                flags: eq.flags.clone(),
                metrics: eq
                    .metric_scores
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            PyMetricScore {
                                normalized: v.normalized,
                                raw: v.raw,
                                warning: v.warning.clone(),
                            },
                        )
                    })
                    .collect(),
            })
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// Python module
// ---------------------------------------------------------------------------

#[pymodule]
fn rosetta_robotics(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyDtype>()?;
    m.add_class::<PyFieldSchema>()?;
    m.add_class::<PyCameraInfo>()?;
    m.add_class::<PyDatasetInfo>()?;
    m.add_class::<PyFrame>()?;
    m.add_class::<PyEpisode>()?;
    m.add_class::<PyConversionResult>()?;
    m.add_class::<PyFormatCapability>()?;
    m.add_class::<PyMetricScore>()?;
    m.add_class::<PyEpisodeQuality>()?;
    m.add_class::<PyQualityReport>()?;
    m.add_function(wrap_pyfunction!(formats, m)?)?;
    m.add_function(wrap_pyfunction!(detect_format, m)?)?;
    m.add_function(wrap_pyfunction!(inspect, m)?)?;
    m.add_function(wrap_pyfunction!(read_episodes, m)?)?;
    m.add_function(wrap_pyfunction!(convert, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_quality, m)?)?;
    Ok(())
}
