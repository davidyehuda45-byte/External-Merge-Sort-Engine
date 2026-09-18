-- TraceOS Database Schema
-- Run on PostgreSQL 16+

-- Enable extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pg_trgm";

-- Organizations & Auth
CREATE TABLE organizations (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(255) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    email VARCHAR(255) NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    role VARCHAR(50) NOT NULL DEFAULT 'ENGINEER',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (org_id, email)
);

CREATE TABLE api_keys (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    key_hash VARCHAR(255) NOT NULL,
    name VARCHAR(255) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at TIMESTAMPTZ
);

CREATE INDEX idx_api_keys_org_id ON api_keys(org_id);
CREATE INDEX idx_api_keys_key_hash ON api_keys(key_hash);

-- Projects & Services
CREATE TABLE projects (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    environment VARCHAR(50) NOT NULL DEFAULT 'production',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_projects_org_id ON projects(org_id);

CREATE TABLE services (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    language VARCHAR(50),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (project_id, name)
);

CREATE INDEX idx_services_project_id ON services(project_id);

-- Telemetry: Logs
CREATE TABLE logs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    service_id UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    timestamp TIMESTAMPTZ NOT NULL,
    level VARCHAR(20) NOT NULL,
    message TEXT NOT NULL,
    trace_id VARCHAR(64),
    request_id VARCHAR(64),
    attributes JSONB DEFAULT '{}',
    fingerprint VARCHAR(64)
);

CREATE INDEX idx_logs_project_id_timestamp ON logs(project_id, timestamp DESC);
CREATE INDEX idx_logs_service_id_timestamp ON logs(service_id, timestamp DESC);
CREATE INDEX idx_logs_trace_id ON logs(trace_id) WHERE trace_id IS NOT NULL;
CREATE INDEX idx_logs_request_id ON logs(request_id) WHERE request_id IS NOT NULL;
CREATE INDEX idx_logs_level ON logs(level);
CREATE INDEX idx_logs_fingerprint ON logs(fingerprint) WHERE fingerprint IS NOT NULL;
CREATE INDEX idx_logs_message_fts ON logs USING GIN (to_tsvector('english', message));

-- Telemetry: Traces
CREATE TABLE traces (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    trace_id VARCHAR(64) NOT NULL,
    root_span_id VARCHAR(32) NOT NULL,
    duration_ms BIGINT,
    status VARCHAR(20) NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    UNIQUE (project_id, trace_id)
);

CREATE INDEX idx_traces_project_id_started_at ON traces(project_id, started_at DESC);
CREATE INDEX idx_traces_trace_id ON traces(trace_id);

-- Telemetry: Spans
CREATE TABLE spans (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    trace_id VARCHAR(64) NOT NULL,
    span_id VARCHAR(32) NOT NULL,
    parent_span_id VARCHAR(32),
    service_id UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    operation_name VARCHAR(255) NOT NULL,
    start_time TIMESTAMPTZ NOT NULL,
    end_time TIMESTAMPTZ NOT NULL,
    duration_ms BIGINT NOT NULL,
    status VARCHAR(20) NOT NULL,
    attributes JSONB DEFAULT '{}'
);

CREATE INDEX idx_spans_trace_id ON spans(trace_id);
CREATE INDEX idx_spans_service_id_start_time ON spans(service_id, start_time DESC);
CREATE INDEX idx_spans_parent_span_id ON spans(parent_span_id) WHERE parent_span_id IS NOT NULL;

-- Telemetry: Metrics
CREATE TABLE metrics (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    service_id UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    metric_name VARCHAR(255) NOT NULL,
    value DOUBLE PRECISION NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    tags JSONB DEFAULT '{}'
);

CREATE INDEX idx_metrics_project_id_service_id_metric_name_timestamp ON metrics(project_id, service_id, metric_name, timestamp DESC);
CREATE INDEX idx_metrics_service_id_metric_name_timestamp ON metrics(service_id, metric_name, timestamp DESC);

-- Deployments
CREATE TABLE deployments (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    service_id UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    version_from VARCHAR(100),
    version_to VARCHAR(100) NOT NULL,
    deployed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deployed_by UUID REFERENCES users(id),
    commit_sha VARCHAR(64)
);

CREATE INDEX idx_deployments_project_id_service_id_deployed_at ON deployments(project_id, service_id, deployed_at DESC);

-- Correlation & Intelligence: Anomalies
CREATE TABLE anomalies (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    service_id UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    metric_name VARCHAR(255) NOT NULL,
    baseline_value DOUBLE PRECISION NOT NULL,
    observed_value DOUBLE PRECISION NOT NULL,
    deviation_pct DOUBLE PRECISION NOT NULL,
    severity VARCHAR(20) NOT NULL,
    detected_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_anomalies_project_id_detected_at ON anomalies(project_id, detected_at DESC);
CREATE INDEX idx_anomalies_service_id_detected_at ON anomalies(service_id, detected_at DESC);

-- Correlation & Intelligence: Incidents
CREATE TABLE incidents (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title VARCHAR(500) NOT NULL,
    severity VARCHAR(20) NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'OPEN',
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMPTZ,
    affected_services JSONB DEFAULT '[]'
);

CREATE INDEX idx_incidents_project_id_status ON incidents(project_id, status);
CREATE INDEX idx_incidents_project_id_started_at ON incidents(project_id, started_at DESC);

CREATE TABLE incident_events (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    incident_id UUID NOT NULL REFERENCES incidents(id) ON DELETE CASCADE,
    event_type VARCHAR(50) NOT NULL,
    event_ref_id UUID,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    description TEXT NOT NULL
);

CREATE INDEX idx_incident_events_incident_id_timestamp ON incident_events(incident_id, timestamp);

CREATE TABLE incident_evidence (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    incident_id UUID NOT NULL REFERENCES incidents(id) ON DELETE CASCADE,
    evidence_type VARCHAR(50) NOT NULL,
    description TEXT NOT NULL,
    confidence_contribution DOUBLE PRECISION NOT NULL,
    raw_data JSONB DEFAULT '{}'
);

CREATE INDEX idx_incident_evidence_incident_id ON incident_evidence(incident_id);

-- Alert Rules
CREATE TABLE alert_rules (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    condition JSONB NOT NULL,
    severity VARCHAR(20) NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_alert_rules_project_id ON alert_rules(project_id);

-- Service Dependencies
CREATE TABLE service_dependencies (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    from_service_id UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    to_service_id UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    call_count BIGINT NOT NULL DEFAULT 0,
    error_count BIGINT NOT NULL DEFAULT 0,
    avg_latency_ms DOUBLE PRECISION NOT NULL DEFAULT 0,
    window_start TIMESTAMPTZ NOT NULL,
    window_end TIMESTAMPTZ NOT NULL,
    UNIQUE (project_id, from_service_id, to_service_id, window_start, window_end)
);

CREATE INDEX idx_service_dependencies_project_id_window ON service_dependencies(project_id, window_start, window_end);

-- Baseline cache table (for anomaly detection)
CREATE TABLE metric_baselines (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    service_id UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    metric_name VARCHAR(255) NOT NULL,
    window_start TIMESTAMPTZ NOT NULL,
    window_end TIMESTAMPTZ NOT NULL,
    avg_value DOUBLE PRECISION NOT NULL,
    stddev_value DOUBLE PRECISION NOT NULL,
    p50_value DOUBLE PRECISION NOT NULL,
    p95_value DOUBLE PRECISION NOT NULL,
    p99_value DOUBLE PRECISION NOT NULL,
    sample_count BIGINT NOT NULL,
    UNIQUE (project_id, service_id, metric_name, window_start, window_end)
);

CREATE INDEX idx_metric_baselines_service_metric_window ON metric_baselines(service_id, metric_name, window_start DESC);

-- Audit log
CREATE TABLE audit_logs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id UUID REFERENCES users(id),
    action VARCHAR(100) NOT NULL,
    resource_type VARCHAR(50),
    resource_id UUID,
    old_data JSONB,
    new_data JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_audit_logs_org_id_created_at ON audit_logs(org_id, created_at DESC);