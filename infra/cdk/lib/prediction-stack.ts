import * as cdk from 'aws-cdk-lib';
import * as ec2 from 'aws-cdk-lib/aws-ec2';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as ecr from 'aws-cdk-lib/aws-ecr';
import * as secretsmanager from 'aws-cdk-lib/aws-secretsmanager';
import * as logs from 'aws-cdk-lib/aws-logs';
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
      '      driver: json-file',
      '      options:',
      '        max-size: "50m"',
      '        max-file: "3"',
      '    healthcheck:',
      '      test: ["CMD", "curl", "-f", "http://localhost:9001/health"]',
      '      interval: 30s',
      '      timeout: 10s',
      '      retries: 3',
      '      start_period: 15s',
      '    restart: "no"',
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
      '# === Enable and start the prediction service ===',
      'systemctl daemon-reload',
      'systemctl enable prediction',
      'systemctl start prediction',
    );

    // === Outputs ===
    new cdk.CfnOutput(this, 'InstanceId', { value: instance.instanceId });
    new cdk.CfnOutput(this, 'EcrRepoUri', { value: ecrRepo.repositoryUri });
    new cdk.CfnOutput(this, 'SecretArn', { value: credentials.secretArn });
    new cdk.CfnOutput(this, 'LogGroupName', { value: logGroup.logGroupName });
    new cdk.CfnOutput(this, 'VpcId', { value: vpc.vpcId });
  }
}
