#!/usr/bin/env python3

import sys

try:
    import paramiko
except ModuleNotFoundError:
    sys.exit(
        """Paramiko not found. Install it via either pip or system package manager to proceed."""
    )

import argparse
from getpass import getpass
from subprocess import CalledProcessError
import csv
import signal
import time
from upgrade.ssh import SSHManager

USERNAME = 'root'


def main(args):
    if args.batch:
        # ssh wrapper may ask for password based on it's own logic, we just provide default
        if args.password:
            password = args.password
        else:
            password = getpass('Default password: ') or ''

        try:
            hosts = [row[0] for row in csv.reader(open(args.batch))]
        except Exception as ex:
            sys.exit('Invalid input file: %s (%s)' % (args.batch, ex))

        if hosts and hosts[0] == 'host':  # possibly skip csv header row
            hosts = hosts[1:]
    else:
        password = args.password or ''
        hosts = [args.hostname]

    error_count = 0
    for host in hosts:
        try:
            update_one(host, password)
        except Exception as ex:
            error_count += 1
            print('Updating %s failed (%s)' % (host, ex))
            if not args.ignore:
                sys.exit(2)
        except CalledProcessError as ex:
            error_count += 1
            print(ex.stdout.read())
            print(ex.stderr.read())
            print('Updating %s failed (%s)' % (host, ex.returncode))
            if not args.ignore:
                sys.exit(3)

    if error_count:
        sys.exit('%d errors encountered' % error_count)


def update_one(host, password):
    print('Updating %s...' % host)
    with SSHManager(host, USERNAME, password) as ssh:
        stdout, stderr = ssh.run('opkg update')
        time.sleep(1)  # opkg may hold lock for a while
        try:
            stdout, stderr = ssh.run('opkg install firmware')
        except CalledProcessError as ex:
            if ex.returncode == -signal.SIGHUP:
                # if all goes well update process reboots which kills ssh server
                return
            raise


def build_arg_parser(parser):
    parser.description = (
        'Trigger firmware update on mining machines running Braiins OS or Braiins OS+'
    )

    parser_sources = parser.add_mutually_exclusive_group(required=True)
    parser_sources.add_argument(
        'hostname', nargs='?', help='hostname of Braiins OS mining machine for '
                                    'installing latest firmware version'
    )
    parser_sources.add_argument(
        '--batch', help='path to file with list of hosts to install to'
    )
    parser.add_argument('-p', '--password', default='', help='Administration password')
    parser.add_argument('-i', '--ignore', action='store_true', help='No halt on errors')


if __name__ == '__main__':
    try:
        parser = argparse.ArgumentParser()
        build_arg_parser(parser)
        args = parser.parse_args()
        main(args)
    except KeyboardInterrupt:
        sys.exit(1)
    except Exception as ex:
        sys.exit('error: %s' % ex)
