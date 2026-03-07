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
          DERIBIT_CLIENT_ID: 'PLACEHOLDER',
          DERIBIT_CLIENT_SECRET: 'PLACEHOLDER',
          DERIVE_WALLET_KEY: 'PLACEHOLDER',
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
    const instance = new ec2.Instance(this, 'Instance', {
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

    // === Outputs ===
    new cdk.CfnOutput(this, 'InstanceId', { value: instance.instanceId });
    new cdk.CfnOutput(this, 'EcrRepoUri', { value: ecrRepo.repositoryUri });
    new cdk.CfnOutput(this, 'SecretArn', { value: credentials.secretArn });
    new cdk.CfnOutput(this, 'LogGroupName', { value: logGroup.logGroupName });
    new cdk.CfnOutput(this, 'VpcId', { value: vpc.vpcId });
  }
}
