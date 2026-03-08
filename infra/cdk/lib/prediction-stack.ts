import * as cdk from 'aws-cdk-lib';
import * as ec2 from 'aws-cdk-lib/aws-ec2';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as ecr from 'aws-cdk-lib/aws-ecr';
import * as secretsmanager from 'aws-cdk-lib/aws-secretsmanager';
import * as logs from 'aws-cdk-lib/aws-logs';
import * as aps from 'aws-cdk-lib/aws-aps';
import * as ssm from 'aws-cdk-lib/aws-ssm';
import * as s3assets from 'aws-cdk-lib/aws-s3-assets';
import * as path from 'path';
import { Construct } from 'constructs';

export class PredictionStack extends cdk.Stack {
  constructor(scope: Construct, id: string, props?: cdk.StackProps) {
    super(scope, id, props);

    // === Network (INFRA-01) ===
    // Single AZ, public subnet only, no NAT gateway ($0/month vs $32/month).
    // Single AZ required because EBS data volume is AZ-pinned.
    const vpc = new ec2.Vpc(this, 'Vpc', {
      maxAzs: 1,
      natGateways: 0,
      subnetConfiguration: [{
        name: 'Public',
        subnetType: ec2.SubnetType.PUBLIC,
        cidrMask: 24,
      }],
    });

    // No inbound rules -- SSM Session Manager is the primary access path.
    // SSH is not exposed; no key pair needed for normal operations.
    const sg = new ec2.SecurityGroup(this, 'InstanceSg', {
      vpc,
      description: 'Prediction EC2 instance security group',
      allowAllOutbound: true,
    });

    // Grafana web UI access (self-hosted Grafana OSS on port 3000)
    sg.addIngressRule(
      ec2.Peer.anyIpv4(),
      ec2.Port.tcp(3000),
      'Grafana web UI access'
    );

    // === ECR Import (INFRA-02) ===
    // Import existing repository by name -- do NOT create a new one.
    // Preserves existing image history and avoids duplicate repos.
    const ecrRepo = ecr.Repository.fromRepositoryName(this, 'EcrRepo', 'prediction');

    // === Secrets Manager (INFRA-07) ===
    // Shell with placeholder keys; real values populated manually post-deploy.
    const credentials = new secretsmanager.Secret(this, 'ApiCredentials', {
      secretName: 'prediction/prod/credentials',
      description: 'Venue API credentials for prediction system',
      generateSecretString: {
        secretStringTemplate: JSON.stringify({
          DERIBIT_API_KEY: 'PLACEHOLDER',
          DERIBIT_API_SECRET: 'PLACEHOLDER',
          POLYMARKET_PRIVATE_KEY: 'PLACEHOLDER',
          KALSHI_API_KEY_ID: 'PLACEHOLDER',
          KALSHI_PRIVATE_KEY: 'PLACEHOLDER',
        }),
        generateStringKey: '_generated',
      },
    });

    // === CloudWatch Logging (INFRA-06) ===
    const logGroup = new logs.LogGroup(this, 'AppLogGroup', {
      logGroupName: '/prediction/production',
      retention: logs.RetentionDays.TWO_WEEKS,
      removalPolicy: cdk.RemovalPolicy.DESTROY,
    });

    // === Amazon Managed Prometheus (MON-02) ===
    const ampWorkspace = new aps.CfnWorkspace(this, 'AmpWorkspace', {
      alias: 'prediction-metrics',
    });

    // === Grafana (MON-03) ===
    // Self-hosted Grafana OSS container replaces Amazon Managed Grafana.
    // AMG was deferred because it requires IAM Identity Center (SSO) subscription.
    // Grafana OSS runs in docker-compose alongside prometheus, uses EC2 instance role
    // for SigV4 auth to query AMP metrics.

    // === SSM Parameter for AMP Workspace ID ===
    const ssmParam = new ssm.StringParameter(this, 'AmpWorkspaceIdParam', {
      parameterName: '/prediction/amp-workspace-id',
      stringValue: ampWorkspace.attrWorkspaceId,
      description: 'AMP workspace ID for Prometheus remote_write configuration',
    });

    // === IAM Instance Profile (INFRA-04) ===
    const instanceRole = new iam.Role(this, 'InstanceRole', {
      assumedBy: new iam.ServicePrincipal('ec2.amazonaws.com'),
      description: 'EC2 instance role for prediction system',
    });

    // SSM for remote management (no SSH needed)
    instanceRole.addManagedPolicy(
      iam.ManagedPolicy.fromAwsManagedPolicyName('AmazonSSMManagedInstanceCore')
    );

    // ECR pull via grant helper (includes GetAuthorizationToken)
    ecrRepo.grantPull(instanceRole);

    // AMP remote write (for future Phase 37 monitoring)
    instanceRole.addManagedPolicy(
      iam.ManagedPolicy.fromAwsManagedPolicyName('AmazonPrometheusRemoteWriteAccess')
    );

    // Secrets Manager read (scoped to this specific secret)
    credentials.grantRead(instanceRole);

    // CloudWatch Logs write (scoped to this specific log group)
    logGroup.grantWrite(instanceRole);

    // CloudWatch Agent for host-level metrics (CPU, memory, disk)
    instanceRole.addManagedPolicy(
      iam.ManagedPolicy.fromAwsManagedPolicyName('CloudWatchAgentServerPolicy')
    );

    // SSM Parameter read (for AMP workspace ID retrieval)
    ssmParam.grantRead(instanceRole);

    // AMP query access (for self-hosted Grafana SigV4 auth via EC2 instance role)
    instanceRole.addToPolicy(new iam.PolicyStatement({
      effect: iam.Effect.ALLOW,
      actions: [
        'aps:QueryMetrics',
        'aps:GetSeries',
        'aps:GetLabels',
        'aps:GetMetricMetadata',
      ],
      resources: [ampWorkspace.attrArn],
    }));

    // === Grafana Provisioning Files (MON-04 through MON-08) ===
    // Dashboard JSON files exceed EC2 user-data 16KB limit, so we upload them
    // as an S3 asset and download during boot. CDK handles the S3 bucket automatically.
    const grafanaAsset = new s3assets.Asset(this, 'GrafanaProvisioningAsset', {
      path: path.join(__dirname, '..', '..', '..', 'grafana', 'provisioning'),
    });
    grafanaAsset.grantRead(instanceRole);

    // === Compute (INFRA-05) ===
    const instance = new ec2.Instance(this, 'Instance2', {
      vpc,
      vpcSubnets: { subnetType: ec2.SubnetType.PUBLIC },
      instanceType: ec2.InstanceType.of(ec2.InstanceClass.T3, ec2.InstanceSize.SMALL),
      machineImage: ec2.MachineImage.latestAmazonLinux2023(),
      role: instanceRole,
      securityGroup: sg,
      associatePublicIpAddress: true,
      blockDevices: [
        {
          deviceName: '/dev/xvda',
          volume: ec2.BlockDeviceVolume.ebs(20, {
            volumeType: ec2.EbsDeviceVolumeType.GP3,
          }),
        },
        {
          deviceName: '/dev/xvdf',
          volume: ec2.BlockDeviceVolume.ebs(30, {
            volumeType: ec2.EbsDeviceVolumeType.GP3,
            deleteOnTermination: false, // CRITICAL: data persists across instance replacement
          }),
        },
      ],
    });

    // IMDSv2 hop limit must be 2 for Docker containers to access instance metadata
    // (default hop limit of 1 blocks containers from reaching the metadata endpoint).
    // Required for Grafana SigV4 auth via EC2 instance role.
    const cfnInstance = instance.node.defaultChild as ec2.CfnInstance;
    cfnInstance.addPropertyOverride('MetadataOptions', {
      HttpEndpoint: 'enabled',
      HttpTokens: 'required',
      HttpPutResponseHopLimit: 2,
    });

    // User-data: format and mount data volume (idempotent)
    instance.userData.addCommands(
      '#!/bin/bash',
      'set -euo pipefail',
      '',
      '# Format data volume only if no filesystem exists (first boot)',
      'if ! blkid /dev/xvdf; then',
      '  mkfs.ext4 /dev/xvdf',
      'fi',
      'mkdir -p /opt/prediction/data',
      'mount /dev/xvdf /opt/prediction/data',
      'echo "/dev/xvdf /opt/prediction/data ext4 defaults,nofail 0 2" >> /etc/fstab',
      '',
      '# Create data subdirectories',
      'mkdir -p /opt/prediction/data/{config,spread_logs,settlement_logs,paper_trades,state,logs}',
    );

    // User-data: install Docker, docker-compose, jq, CloudWatch agent, and bootstrap services
    instance.userData.addCommands(
      '',
      '# === Install Docker (AL2023 uses dnf) ===',
      'dnf install -y docker',
      'systemctl enable docker',
      'systemctl start docker',
      '',
      '# === Install docker-compose v2 plugin ===',
      'mkdir -p /usr/local/lib/docker/cli-plugins',
      'curl -SL "https://github.com/docker/compose/releases/latest/download/docker-compose-linux-x86_64" \\',
      '  -o /usr/local/lib/docker/cli-plugins/docker-compose',
      'chmod +x /usr/local/lib/docker/cli-plugins/docker-compose',
      '',
      '# === Install jq for secrets parsing ===',
      'dnf install -y jq',
      '',
      '# === Install and configure CloudWatch agent ===',
      'dnf install -y amazon-cloudwatch-agent',
      '',
      'cat > /opt/aws/amazon-cloudwatch-agent/etc/amazon-cloudwatch-agent.json <<\'CWEOF\'',
      '{',
      '  "agent": {',
      '    "metrics_collection_interval": 60,',
      '    "run_as_user": "root"',
      '  },',
      '  "metrics": {',
      '    "namespace": "Prediction/EC2",',
      '    "metrics_collected": {',
      '      "cpu": {',
      '        "measurement": ["cpu_usage_idle", "cpu_usage_user", "cpu_usage_system"],',
      '        "totalcpu": true',
      '      },',
      '      "mem": {',
      '        "measurement": ["mem_used_percent", "mem_available_percent"]',
      '      },',
      '      "disk": {',
      '        "measurement": ["used_percent", "free"],',
      '        "resources": ["/", "/opt/prediction/data"]',
      '      }',
      '    }',
      '  }',
      '}',
      'CWEOF',
      '',
      '/opt/aws/amazon-cloudwatch-agent/bin/amazon-cloudwatch-agent-ctl -a fetch-config -m ec2 -s -c file:/opt/aws/amazon-cloudwatch-agent/etc/amazon-cloudwatch-agent.json',
      '',
      '# === Create fetch-secrets.sh ===',
      'cat > /opt/prediction/fetch-secrets.sh <<\'FSEOF\'',
      '#!/bin/bash',
      'set -euo pipefail',
      '',
      'SECRET_JSON=$(aws secretsmanager get-secret-value \\',
      '  --secret-id prediction/prod/credentials \\',
      '  --region us-east-1 \\',
      '  --query SecretString --output text)',
      '',
      'cat > /opt/prediction/.env <<ENV',
      'DERIBIT_API_KEY=$(echo "$SECRET_JSON" | jq -r ".DERIBIT_API_KEY // empty")',
      'DERIBIT_API_SECRET=$(echo "$SECRET_JSON" | jq -r ".DERIBIT_API_SECRET // empty")',
      'POLYMARKET_PRIVATE_KEY=$(echo "$SECRET_JSON" | jq -r ".POLYMARKET_PRIVATE_KEY // empty")',
      'KALSHI_API_KEY_ID=$(echo "$SECRET_JSON" | jq -r ".KALSHI_API_KEY_ID // empty")',
      'KALSHI_PRIVATE_KEY=$(echo "$SECRET_JSON" | jq -r ".KALSHI_PRIVATE_KEY // empty")',
      'ENV',
      '',
      'chmod 600 /opt/prediction/.env',
      '',
      '# ECR login for docker pull',
      'aws ecr get-login-password --region us-east-1 | docker login --username AWS --password-stdin 606103597377.dkr.ecr.us-east-1.amazonaws.com',
      'FSEOF',
      '',
      'chmod +x /opt/prediction/fetch-secrets.sh',
      '',
      '# === Retrieve AMP workspace ID and write Prometheus config ===',
      'AMP_WORKSPACE_ID=$(aws ssm get-parameter --name /prediction/amp-workspace-id --region us-east-1 --query Parameter.Value --output text)',
      '',
      'cat > /opt/prediction/prometheus.yml <<PROMEOF',
      'global:',
      '  scrape_interval: 15s',
      '  evaluation_interval: 15s',
      '  external_labels:',
      '    cluster: prediction-prod',
      '    instance: ec2',
      '',
      'scrape_configs:',
      '  - job_name: prediction',
      '    static_configs:',
      "      - targets: ['prediction:9000']",
      '    scrape_interval: 15s',
      '    metrics_path: /metrics',
      '',
      'remote_write:',
      '  - url: https://aps-workspaces.us-east-1.amazonaws.com/workspaces/${AMP_WORKSPACE_ID}/api/v1/remote_write',
      '    queue_config:',
      '      max_samples_per_send: 1000',
      '      max_shards: 200',
      '      capacity: 2500',
      '    sigv4:',
      '      region: us-east-1',
      'PROMEOF',
      '',
      '# === Write Grafana provisioning config ===',
      'mkdir -p /opt/prediction/grafana/provisioning/{datasources,dashboards/json,alerting}',
      '',
      '# --- Data source: AMP (with stable uid for dashboard references) ---',
      '# amp.yml uses shell variable AMP_WORKSPACE_ID so it must be written via heredoc.',
      'cat > /opt/prediction/grafana/provisioning/datasources/amp.yml <<GRAFEOF',
      'apiVersion: 1',
      '',
      'datasources:',
      '  - name: AMP',
      '    uid: amp',
      '    type: prometheus',
      '    access: proxy',
      '    url: https://aps-workspaces.us-east-1.amazonaws.com/workspaces/${AMP_WORKSPACE_ID}/',
      '    isDefault: true',
      '    jsonData:',
      '      httpMethod: POST',
      '      sigV4Auth: true',
      '      sigV4AuthType: default',
      '      sigV4Region: us-east-1',
      '    editable: true',
      'GRAFEOF',
      '',
      '# --- Download dashboard JSON, alerting YAML, and provider config from S3 asset ---',
      '# Dashboard JSON files exceed EC2 user-data 16KB limit, so they are uploaded',
      '# as a CDK S3 asset and downloaded+extracted during boot.',
      'dnf install -y unzip',
    );
    // Use CDK's S3 download helper to inject the correct S3 bucket/key as CloudFormation refs
    const grafanaLocalPath = instance.userData.addS3DownloadCommand({
      bucket: grafanaAsset.bucket,
      bucketKey: grafanaAsset.s3ObjectKey,
      localFile: '/tmp/grafana-provisioning.zip',
    });
    instance.userData.addCommands(
      'cd /tmp && unzip -o grafana-provisioning.zip -d grafana-provisioning',
      '# Copy all provisioning files except amp.yml (already written with AMP_WORKSPACE_ID above)',
      'cp -r /tmp/grafana-provisioning/dashboards /opt/prediction/grafana/provisioning/',
      'cp -r /tmp/grafana-provisioning/alerting /opt/prediction/grafana/provisioning/',
      'rm -rf /tmp/grafana-provisioning /tmp/grafana-provisioning.zip',
      '',
      '# === Write docker-compose.yml ===',
      'cat > /opt/prediction/docker-compose.yml <<\'DCEOF\'',
      'services:',
      '  prediction:',
      '    image: 606103597377.dkr.ecr.us-east-1.amazonaws.com/prediction:latest',
      '    env_file: .env',
      '    stop_grace_period: 30s',
      '    ports:',
      '      - "9000:9000"',
      '      - "9001:9001"',
      '    volumes:',
      '      - /opt/prediction/data/config:/app/config',
      '      - /opt/prediction/data/spread_logs:/app/spread_logs',
      '      - /opt/prediction/data/settlement_logs:/app/settlement_logs',
      '      - /opt/prediction/data/paper_trades:/app/paper_trades',
      '      - /opt/prediction/data/state:/app/state',
      '      - /opt/prediction/data/logs:/app/logs',
      '    logging:',
      '      driver: awslogs',
      '      options:',
      '        awslogs-region: us-east-1',
      '        awslogs-group: /prediction/production',
      '        tag: prediction',
      '        mode: non-blocking',
      '        max-buffer-size: 4m',
      '    healthcheck:',
      '      test: ["CMD", "curl", "-f", "http://localhost:9001/health"]',
      '      interval: 30s',
      '      timeout: 10s',
      '      retries: 3',
      '      start_period: 15s',
      '    restart: "no"',
      '  prometheus:',
      '    image: prom/prometheus:v3.10.0',
      '    command:',
      "      - '--config.file=/etc/prometheus/prometheus.yml'",
      "      - '--storage.tsdb.retention.time=2h'",
      "      - '--web.enable-lifecycle'",
      '    volumes:',
      '      - /opt/prediction/prometheus.yml:/etc/prometheus/prometheus.yml:ro',
      '    depends_on:',
      '      prediction:',
      '        condition: service_healthy',
      '    restart: "no"',
      '  grafana:',
      '    image: grafana/grafana-oss:11.5.2',
      '    ports:',
      '      - "3000:3000"',
      '    environment:',
      '      - GF_SECURITY_ADMIN_PASSWORD=admin',
      '      - GF_AUTH_SIGV4_AUTH_ENABLED=true',
      '      - AWS_SDK_LOAD_CONFIG=true',
      '    volumes:',
      '      - /opt/prediction/grafana/provisioning:/etc/grafana/provisioning:ro',
      '      - grafana-data:/var/lib/grafana',
      '    depends_on:',
      '      - prometheus',
      '    restart: "no"',
      'volumes:',
      '  grafana-data:',
      'DCEOF',
      '',
      '# === Create systemd unit ===',
      'cat > /etc/systemd/system/prediction.service <<\'SDEOF\'',
      '[Unit]',
      'Description=Prediction Market Arbitrage System',
      'After=docker.service network-online.target',
      'Requires=docker.service',
      'Wants=network-online.target',
      '',
      '[Service]',
      'Type=simple',
      'WorkingDirectory=/opt/prediction',
      'ExecStartPre=/opt/prediction/fetch-secrets.sh',
      'ExecStart=/usr/bin/docker compose up --no-build',
      'ExecStop=/usr/bin/docker compose down',
      'Restart=on-failure',
      'RestartSec=10',
      'TimeoutStopSec=45',
      'Environment=AWS_DEFAULT_REGION=us-east-1',
      '',
      '[Install]',
      'WantedBy=multi-user.target',
      'SDEOF',
      '',
      '# === Ensure stdout_json=true for CloudWatch JSON log ingestion ===',
      'if [ -f /opt/prediction/data/config/config.toml ]; then',
      '  grep -q "stdout_json" /opt/prediction/data/config/config.toml || sed -i "/^\\[logging\\]/a stdout_json = true" /opt/prediction/data/config/config.toml',
      'fi',
      '',
      '# === Enable and start the prediction service ===',
      'systemctl daemon-reload',
      'systemctl enable prediction',
      'systemctl start prediction',
    );

    // === CI/CD Deploy User (CICD-01) ===
    // IAM user for GitLab CI/CD pipeline -- access keys created manually in IAM console.
    // Do NOT create access keys in CDK (they'd appear in CloudFormation outputs in plaintext).
    const ciDeployUser = new iam.User(this, 'CiDeployUser', {
      userName: 'prediction-ci-deploy',
    });

    // ECR push permissions (scoped to prediction repo)
    ciDeployUser.addToPolicy(new iam.PolicyStatement({
      sid: 'ECRAuth',
      effect: iam.Effect.ALLOW,
      actions: ['ecr:GetAuthorizationToken'],
      resources: ['*'], // Required by AWS -- GetAuthorizationToken does not support resource scoping
    }));

    ciDeployUser.addToPolicy(new iam.PolicyStatement({
      sid: 'ECRPush',
      effect: iam.Effect.ALLOW,
      actions: [
        'ecr:BatchCheckLayerAvailability',
        'ecr:InitiateLayerUpload',
        'ecr:UploadLayerPart',
        'ecr:CompleteLayerUpload',
        'ecr:PutImage',
      ],
      resources: [ecrRepo.repositoryArn],
    }));

    // SSM deploy permissions (send commands to EC2 instance)
    ciDeployUser.addToPolicy(new iam.PolicyStatement({
      sid: 'SSMSendCommand',
      effect: iam.Effect.ALLOW,
      actions: ['ssm:SendCommand'],
      resources: [
        `arn:aws:ec2:${this.region}:${this.account}:instance/${instance.instanceId}`,
        `arn:aws:ssm:${this.region}::document/AWS-RunShellScript`,
      ],
    }));

    ciDeployUser.addToPolicy(new iam.PolicyStatement({
      sid: 'SSMCommandStatus',
      effect: iam.Effect.ALLOW,
      actions: [
        'ssm:GetCommandInvocation',
        'ssm:ListCommandInvocations',
      ],
      resources: ['*'],
    }));

    // === Outputs ===
    new cdk.CfnOutput(this, 'InstanceId', { value: instance.instanceId });
    new cdk.CfnOutput(this, 'EcrRepoUri', { value: ecrRepo.repositoryUri });
    new cdk.CfnOutput(this, 'SecretArn', { value: credentials.secretArn });
    new cdk.CfnOutput(this, 'LogGroupName', { value: logGroup.logGroupName });
    new cdk.CfnOutput(this, 'VpcId', { value: vpc.vpcId });
    new cdk.CfnOutput(this, 'AmpWorkspaceId', { value: ampWorkspace.attrWorkspaceId });
    new cdk.CfnOutput(this, 'AmpPrometheusEndpoint', { value: ampWorkspace.attrPrometheusEndpoint });
    new cdk.CfnOutput(this, 'CiDeployUserName', {
      value: ciDeployUser.userName,
      description: 'IAM user for GitLab CI/CD pipeline -- create access keys in IAM console',
    });
    // Grafana is self-hosted on EC2 port 3000 (no separate endpoint output needed)
  }
}
